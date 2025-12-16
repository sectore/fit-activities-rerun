import argparse
import datetime
import os
from dataclasses import dataclass
from pathlib import Path
from typing import Optional, TypeAlias

import fitdecode
import rerun as rr
import rerun.blueprint as rrb
from dotenv import load_dotenv

Position: TypeAlias = tuple[float, float]


def format_distance(kilometers: float) -> str:
    """Convert kilometers to readable format (e.g. 500m, 1.2km)."""
    meters = kilometers * 1000

    if meters < 1000:
        return f"{int(meters)}m"

    return f"{kilometers:.1f}km"


def format_time(seconds: float) -> str:
    """Convert seconds to humanized format (e.g. 2s, 12m 39s, 1h 22m 23s, 2d 1h 5m 3s)."""
    secs = int(seconds)

    if secs < 60:
        return f"{secs}s"

    parts = []

    if secs >= 86400:
        days, secs = divmod(secs, 86400)
        parts.append(f"{days}d")

    if secs >= 3600:
        hours, secs = divmod(secs, 3600)
        parts.append(f"{hours}h")

    if secs >= 60:
        minutes, secs = divmod(secs, 60)
        parts.append(f"{minutes}m")

    if secs > 0:
        parts.append(f"{secs}s")

    return " ".join(parts)


@dataclass
class Record:
    timestamp: datetime.datetime
    position_lat: Optional[float] = None
    position_long: Optional[float] = None
    distance: Optional[float] = None
    speed: Optional[float] = None
    heartrate: Optional[int] = None
    temperature: Optional[int] = None
    altitude: Optional[float] = None


@dataclass
class Activity:
    id: str
    records: list[Record]
    type: Optional[str] = None

    # time data
    start_time: Optional[datetime.datetime] = None
    total_time: Optional[float] = None
    pause_time: Optional[float] = None

    # distance data
    total_distance: Optional[float] = None

    # temperature data
    no_temperature_records: int = False
    max_temperature: Optional[int] = None
    min_temperature: Optional[int] = None
    avg_temperature: Optional[int] = None

    def has_temperature_data(self) -> bool:
        return self.no_temperature_records > 0

    # altitude data
    no_altitude_records: int = 0
    max_altitude: Optional[float] = None
    min_altitude: Optional[float] = None
    avg_altitude: Optional[float] = None

    def has_altitude_data(self) -> bool:
        return self.no_altitude_records > 0

    # speed data
    no_speed_records: int = 0
    max_speed: Optional[float] = None
    min_speed: Optional[float] = None
    avg_speed: Optional[float] = None

    def has_speed_data(self) -> bool:
        return self.no_speed_records > 0

    # heartrate data
    max_heartrate: Optional[int] = None
    min_heartrate: Optional[int] = None
    avg_heartrate: Optional[int] = None
    no_heartrate_records: int = 0

    def has_heartrate_data(self) -> bool:
        return self.no_heartrate_records > 0


def get_available_data_ids(act: Activity) -> list[str]:
    """Return list of available data identifiers for the activity."""
    return [
        data_id
        for data_id, has_data in [
            ("speed", act.has_speed_data()),
            ("heartrate", act.has_heartrate_data()),
            ("altitude", act.has_altitude_data()),
            ("temperature", act.has_temperature_data()),
        ]
        if has_data
    ]


def blueprint_default() -> rrb.BlueprintLike:
    """Default application blueprint."""
    return rrb.Blueprint(auto_views=True, auto_layout=True)


def blueprint_vertical(act: Activity, use_mapbox: bool) -> rrb.BlueprintLike:
    """Custom blueprint to visualize `Activity` data."""

    id = act.id

    data_ids = get_available_data_ids(act)
    viewport = rrb.Vertical(
        contents=[
            rrb.MapView(
                name="map",
                origin=f"{id}/route",
                background=rrb.MapProvider.MapboxDark
                if use_mapbox
                else rrb.MapProvider.OpenStreetMap,
            ),
            rrb.Horizontal(
                contents=[rrb.TextDocumentView(name="info", origin=f"{id}/info")]
                + [
                    rrb.TimeSeriesView(name=f"{data_id}", origin=f"{id}/{data_id}")
                    for data_id in data_ids
                ],
            ),
        ],
        row_shares=[3, 1],
    )

    return rrb.Blueprint(
        viewport,
        rrb.TimePanel(state="collapsed"),
        rrb.SelectionPanel(state="collapsed"),
        rrb.BlueprintPanel(expanded=False),
    )


def parse_fit_file(file_path: Path) -> Activity:
    """Parse FIT file and extract `RecordData`."""
    records = []

    id = file_path.stem.replace(" ", "_")
    activity = Activity(id=id, records=[])

    with fitdecode.FitReader(
        file_path, processor=fitdecode.StandardUnitsDataProcessor()
    ) as fit:
        for frame in fit:
            if isinstance(frame, fitdecode.FitDataMessage):
                if frame.name == "session":
                    print("Session fields:")
                    for field in frame.fields:
                        value = frame.get_value(field.name, fallback=None)
                        print(f"  {field.name}: {value} ({type(value).__name__})")

                    if isinstance(
                        value := frame.get_value("sport", fallback=None), str
                    ):
                        activity.type = value
                    if isinstance(
                        value := frame.get_value("start_time", fallback=None),
                        datetime.datetime,
                    ):
                        activity.start_time = value
                    if isinstance(
                        value := frame.get_value("total_distance", fallback=None), float
                    ):
                        activity.total_distance = value
                    if isinstance(
                        value := frame.get_value("enhanced_max_speed", fallback=None)
                        or frame.get_value("max_speed", fallback=None),
                        float,
                    ):
                        activity.max_speed = value
                    if isinstance(
                        value := frame.get_value("enhanced_avg_speed", fallback=None)
                        or frame.get_value("avg_speed", fallback=None),
                        float,
                    ):
                        activity.avg_speed = value

                    if isinstance(
                        value := frame.get_value("max_temperature", fallback=None), int
                    ):
                        activity.max_temperature = value
                    if isinstance(
                        value := frame.get_value("avg_temperature", fallback=None), int
                    ):
                        activity.avg_temperature = value
                    if isinstance(
                        value := frame.get_value("enhanced_max_altitude", fallback=None)
                        or frame.get_value("max_altitude", fallback=None),
                        float,
                    ):
                        activity.max_altitude = value
                    if isinstance(
                        value := frame.get_value("enhanced_min_altitude", fallback=None)
                        or frame.get_value("min_altitude", fallback=None),
                        float,
                    ):
                        activity.min_altitude = value
                    if isinstance(
                        value := frame.get_value("enhanced_avg_altitude", fallback=None)
                        or frame.get_value("avg_altitude", fallback=None),
                        float,
                    ):
                        activity.avg_altitude = value
                    if isinstance(
                        value := frame.get_value("max_heart_rate", fallback=None), int
                    ):
                        activity.max_heartrate = value

                    if isinstance(
                        value := frame.get_value("avg_heart_rate", fallback=None), int
                    ):
                        activity.avg_heartrate = value

                    if isinstance(
                        value := frame.get_value("total_elapsed_time", fallback=None),
                        float,
                    ):
                        activity.total_time = value

                    total_timer_time = frame.get_value(
                        "total_timer_time", fallback=None
                    )
                    if (
                        isinstance(total_timer_time, float)
                        and activity.total_time is not None
                    ):
                        activity.pause_time = activity.total_time - total_timer_time

                elif frame.name == "record":
                    if not isinstance(
                        timestamp := frame.get_value("timestamp", fallback=None),
                        datetime.datetime,
                    ):
                        continue

                    record = Record(timestamp=timestamp)

                    if isinstance(
                        value := frame.get_value("distance", fallback=None), float
                    ):
                        record.distance = value

                    if isinstance(
                        value := frame.get_value("heart_rate", fallback=None), int
                    ):
                        activity.no_heartrate_records += 1
                        record.heartrate = value
                        activity.min_heartrate = (
                            value
                            if activity.min_heartrate is None
                            else min(activity.min_heartrate, value)
                        )

                    if isinstance(
                        value := frame.get_value("temperature", fallback=None), int
                    ):
                        activity.no_temperature_records += 1
                        record.temperature = value
                        activity.min_temperature = (
                            value
                            if activity.min_temperature is None
                            else min(activity.min_temperature, value)
                        )

                    if isinstance(
                        value := frame.get_value("enhanced_speed", fallback=None)
                        or frame.get_value("speed", fallback=None),
                        float,
                    ):
                        activity.no_speed_records += 1
                        record.speed = value
                        activity.min_speed = (
                            value
                            if activity.min_speed is None
                            else min(activity.min_speed, value)
                        )

                    if isinstance(
                        value := frame.get_value("enhanced_altitude", fallback=None)
                        or frame.get_value("altitude", fallback=None),
                        float,
                    ):
                        activity.no_altitude_records += 1
                        record.altitude = value

                    if isinstance(
                        value := frame.get_value("position_lat", fallback=None), float
                    ):
                        record.position_lat = value

                    if isinstance(
                        value := frame.get_value("position_long", fallback=None), float
                    ):
                        record.position_long = value

                    records.append(record)

    activity.records = records

    return activity


def log_data(act: Activity):
    activity_positions: list[Position] = []
    route_positions: list[Position] = []

    id = act.id

    data_ids = get_available_data_ids(act)

    # style scalar lines + peak points
    # Note: 7F -> 127 for transparency (50%)
    for data_id in data_ids:
        # lines
        rr.log(
            f"{id}/{data_id}/value",
            rr.SeriesLines(names="values", widths=2),
            static=True,
        )
        # avg
        rr.log(
            f"{id}/{data_id}/avg",
            rr.SeriesLines(names="avg", colors=0x1B7EF77F, widths=2),
            static=True,
        )
        # peak
        rr.log(
            f"{id}/{data_id}/peak",
            rr.SeriesPoints(
                names="peak", colors=0xF71B1B7F, markers="circle", marker_sizes=5
            ),
            static=True,
        )

    info_lines = [f"### {(act.type or 'Summary').upper()}"]

    info_lines.append(f"- ID **{act.id}**")
    info_lines.append(f"- NO. RECORDS **{len(act.records)}**")

    info_lines.append("\n")
    info_lines.append("###### **SESSION SUMMARY**")

    if act.start_time is not None:
        start_time = act.start_time.strftime("%d.%m.%Y %H:%M:%S")
        info_lines.append(f"- START **{start_time}**")
    if act.total_time is not None or act.pause_time is not None:
        parts = []
        if act.total_time is not None:
            parts.append(f"**{format_time(act.total_time)}** (total)")
        if act.pause_time is not None and act.pause_time > 0:
            parts.append(f"**{format_time(act.pause_time)}** (pause)")
        info_lines.append(f"- DURATION {' '.join(parts)}")
    if act.total_distance is not None:
        info_lines.append(f"- DISTANCE **{format_distance(act.total_distance)}**")

    info_lines.append("\n")
    info_lines.append("###### **RECORDS SUMMARY**")
    info_lines.append("|  | max | min | avg | no. rec.")
    info_lines.append("| --- | --- | --- | --- | --- |")
    info_empty_col = "-- |"

    if act.has_speed_data():
        row = "|SPEED|"
        if act.max_speed is not None:
            row += f"**{act.max_speed:.2f} km/h**|"
        else:
            row += info_empty_col
        if act.min_speed is not None:
            row += f"**{act.min_speed:.2f} km/h**|"
        else:
            row += info_empty_col
        if act.avg_speed is not None:
            row += f"**{act.avg_speed:.2f} km/h**|"
        else:
            row += info_empty_col

        row += f"**{act.no_speed_records}**|"
        info_lines.append(row)

    if act.has_heartrate_data():
        row = "|♥ RATE|"
        if act.max_heartrate is not None:
            row += f"**{act.max_heartrate} bpm**|"
        else:
            row += info_empty_col
        if act.min_heartrate is not None:
            row += f"**{act.min_heartrate} bpm**|"
        else:
            row += info_empty_col
        if act.avg_heartrate is not None:
            row += f"**{act.avg_heartrate} bpm**|"
        else:
            row += info_empty_col

        row += f"**{act.no_heartrate_records}**|"
        info_lines.append(row)

    if act.has_altitude_data():
        row = "|ALTITUDE |"
        if act.max_altitude is not None:
            row += f"**{act.max_altitude:.0f} m**|"
        else:
            row += info_empty_col
        if act.min_altitude is not None:
            row += f"**{act.min_altitude:.0f} m**|"
        else:
            row += info_empty_col
        if act.avg_altitude is not None:
            row += f"**{act.avg_altitude:.0f} m**|"
        else:
            row += info_empty_col

        row += f"**{act.no_altitude_records}**|"
        info_lines.append(row)

    if act.has_temperature_data():
        row = "|TEMPERATURE|"
        if act.max_temperature is not None:
            row += f"**{act.max_temperature} °C**|"
        else:
            row += info_empty_col
        if act.min_temperature is not None:
            row += f"**{act.min_temperature} °C**|"
        else:
            row += info_empty_col
        if act.avg_temperature is not None:
            row += f"**{act.avg_temperature} °C**|"
        else:
            row += info_empty_col

        row += f"**{act.no_temperature_records}**|"
        info_lines.append(row)

    info_md = "\n".join(info_lines)

    rr.log(
        f"{id}/info",
        rr.TextDocument(info_md, media_type=rr.MediaType.MARKDOWN),
        static=True,
    )

    # get geo data from records
    for record in act.records:
        if record.position_lat is not None and record.position_long is not None:
            p: Position = (record.position_lat, record.position_long)
            activity_positions.append(p)

    # show all route
    rr.log(
        f"{id}/route/all",
        rr.GeoLineStrings(
            lat_lon=activity_positions,
            radii=rr.Radius.ui_points(1.5),
            colors=[255, 255, 255],
        ),
        (),
        static=True,
    )

    for record in act.records:
        rr.set_time(f"{id}/time", timestamp=record.timestamp)

        if record.position_lat is not None and record.position_long is not None:
            pos: Position = (record.position_lat, record.position_long)
            route_positions.append(pos)

            # log route of current record
            rr.log(
                f"{id}/route/current",
                rr.GeoLineStrings(
                    lat_lon=route_positions,
                    radii=rr.Radius.ui_points(1.5),
                    colors=[247, 147, 26],
                ),
            )

            # log point of current record
            rr.log(
                f"{id}/route/destination",
                rr.GeoPoints(
                    lat_lon=pos,
                    radii=rr.Radius.ui_points(6.0),
                    colors=[247, 147, 26],
                ),
            )

        # log record data: value / max / avg
        for data_id in data_ids:
            value = getattr(record, data_id, None)
            if value is not None:
                rr.log(f"{id}/{data_id}", rr.Scalars(value))
                max_value = getattr(act, f"max_{data_id}", None)
                if max_value is not None and max_value == value:
                    rr.log(f"{id}/{data_id}/peak", rr.Scalars(max_value))
                avg_value = getattr(act, f"avg_{data_id}", None)
                if avg_value is not None:
                    rr.log(f"{id}/{data_id}/avg", rr.Scalars(avg_value))


def main():
    load_dotenv()

    parser = argparse.ArgumentParser(description="Visualize `*.fit` data using Rerun")
    parser.add_argument(
        "--fit",
        type=str,
        help="Path to the .fit file",
    )
    parser.add_argument(
        "--blueprint",
        choices=["none", "vertical"],
        default="vertical",
        help="Select the blueprint to use",
    )
    rr.script_add_args(parser)

    args = parser.parse_args()

    # Load fit file with validation
    file_path = args.fit
    if args.fit:
        if not file_path.lower().endswith(".fit"):
            print(f"Error: File must have .fit extension, got '{file_path}'")
            return
        if not Path(file_path).exists():
            print(f"Error: File does not exist: '{file_path}'")
            return
        file_path = Path(file_path)
    else:
        print(f"Invalid file or does not exist: {file_path}")
        return

    # parse data
    activity = parse_fit_file(file_path)

    if args.blueprint == "vertical":
        use_mapbox = "RERUN_MAPBOX_ACCESS_TOKEN" in os.environ
        blueprint = blueprint_vertical(activity, use_mapbox)
    else:
        blueprint = blueprint_default()

    # rr.init("fit_activities_rerun", spawn=True)
    rr.script_setup(args, "fit_activities_rerun")
    rr.send_blueprint(blueprint)

    log_data(activity)

    rr.script_teardown(args)


if __name__ == "__main__":
    main()
