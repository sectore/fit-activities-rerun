use anyhow::{Context, Result};
use chrono::{DateTime, Local};
use clap::{Parser, ValueEnum};
use fitparser::de::DecodeOption;
use rerun::{
    GeoLineStrings, GeoPoints, MediaType, Radius, RecordingStream, Scalars, SeriesLines,
    SeriesPoints, TextDocument,
    blueprint::*,
    components::{Color, MarkerShape},
    external::re_sdk_types::blueprint::components::MapProvider,
};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, Copy, Default, ValueEnum)]
#[value(rename_all = "lowercase")]
enum MapStyle {
    /// OpenStreetMap
    #[default]
    Osm,
    /// Dark style (Mapbox)
    Dark,
    /// Light style (Mapbox)
    Light,
    /// Streets style (Mapbox)
    Streets,
    /// Satellite style (Mapbox)
    Satellite,
}

#[derive(Parser, Debug)]
#[command(
    name = "fit-activities-rerun",
    about = "Visualize `*.fit` data using Rerun.",
    version
)]
struct Args {
    #[command(flatten)]
    rerun: rerun::clap::RerunArgs,
    /// Path to the .fit file
    #[clap(long, value_name = "FILE", required = true)]
    fit: PathBuf,
    /// Map tile style. To use styles other than 'osm', set the environment variable RERUN_MAPBOX_ACCESS_TOKEN to enable Mapbox instead.
    #[clap(long, value_enum, default_value_t)]
    map: MapStyle,
    /// Blueprint layout to use
    #[clap(long, value_enum, default_value_t)]
    blueprint: BlueprintChoice,
}

#[derive(Debug, Clone, Copy, Default, ValueEnum)]
#[value(rename_all = "lowercase")]
enum BlueprintChoice {
    /// No custom blueprint (auto-layout)
    None,
    /// Default horizontal layout (Map left, Info+Metrics right)
    Default,
    /// Vertical layout (Map top, Info+Metrics bottom)
    #[default]
    Vertical,
}

#[derive(Debug, Default)]
struct TimeStats {
    start: Option<DateTime<Local>>,
    total: Option<Duration>,
    pause: Option<Duration>,
}

fn format_time(duration: &Duration) -> String {
    let seconds = duration.as_secs();

    const MINUTE: u64 = 60;
    const HOUR: u64 = 60 * MINUTE;
    const DAY: u64 = 24 * HOUR;

    if seconds < MINUTE {
        return format!("{seconds}s");
    }

    let (days, remainder) = (seconds / DAY, seconds % DAY);
    let (hours, remainder) = (remainder / HOUR, remainder % HOUR);
    let (minutes, secs) = (remainder / MINUTE, remainder % MINUTE);

    [(days, "d"), (hours, "h"), (minutes, "m"), (secs, "s")]
        .into_iter()
        .filter(|(value, _)| *value > 0)
        .map(|(value, unit)| format!("{value}{unit}"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Debug, Clone, Copy)]
struct LatLong(f64);

impl LatLong {
    // semicircles to degrees.
    // Based on formulas: degrees = semicircles × (180 / 2^31)
    // @see https://learn.microsoft.com/en-us/previous-versions/windows/embedded/cc510650(v=msdn.10)
    const SEMICIRCLE_CONVERSION_FACTOR: f64 = 180.0 / (1u64 << 31) as f64;

    pub fn from_semicircle(semicircle: i32) -> Self {
        LatLong(semicircle as f64 * Self::SEMICIRCLE_CONVERSION_FACTOR)
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, PartialOrd)]
struct Speed(f64);

impl Speed {
    fn format(&self) -> String {
        let km_per_hour = self.0 * 3.6;
        format!("{:.2} km/h", km_per_hour).replace(".00", "")
    }

    // Note: `min`/`max` can't be derived (f64 doesn't implement Ord due to NaN)
    pub fn min(self, other: Self) -> Self {
        Speed(self.0.min(other.0))
    }

    pub fn max(self, other: Self) -> Self {
        Speed(self.0.max(other.0))
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Heartrate(u8);

impl Heartrate {
    fn format(&self) -> String {
        format!("{} bpm", self.0)
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Temperature(i8);

impl Temperature {
    fn format(&self) -> String {
        format!("{} °C", self.0)
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, PartialOrd)]
struct Altitude(f64);

impl Altitude {
    fn format(&self) -> String {
        format!("{:.0} m", self.0)
    }

    pub fn min(self, other: Self) -> Self {
        Altitude(self.0.min(other.0))
    }

    pub fn max(self, other: Self) -> Self {
        Altitude(self.0.max(other.0))
    }
}

#[derive(Debug, Default)]
struct Distance(f64);

impl Distance {
    fn from_unscaled(value: f64) -> Self {
        Self(value * 100.0)
    }

    fn format(&self) -> String {
        let meters = self.0;
        if meters < 1000.0 {
            format!("{}m", meters as u32)
        } else {
            format!("{:.1}km", meters / 1000.0)
        }
    }
}

#[derive(Debug, Default)]
struct Record {
    timestamp: DateTime<Local>,
    position_lat: Option<LatLong>,
    position_long: Option<LatLong>,
    distance: Option<Distance>,
    speed: Option<Speed>,
    heartrate: Option<Heartrate>,
    temperature: Option<Temperature>,
    altitude: Option<Altitude>,
}

#[derive(Debug, Default)]
struct Stats<T> {
    no_records: usize,
    max: Option<T>,
    min: Option<T>,
    avg: Option<T>,
}

// Type aliases for domain-specific stats
type TemperatureStats = Stats<Temperature>;
type AltitudeStats = Stats<Altitude>;
type SpeedStats = Stats<Speed>;
type HeartrateStats = Stats<Heartrate>;

#[derive(Debug)]
struct Activity {
    id: String,
    records: Vec<Record>,
    activity_type: Option<String>,
    time_stats: TimeStats,
    total_distance: Option<Distance>,
    temp_stats: TemperatureStats,
    altitude_stats: AltitudeStats,
    speed_stats: SpeedStats,
    heartrate_stats: HeartrateStats,
}

impl Activity {
    fn new(id: String) -> Self {
        Self {
            id,
            records: Vec::new(),
            activity_type: None,
            time_stats: Default::default(),
            total_distance: None,
            temp_stats: Default::default(),
            altitude_stats: Default::default(),
            speed_stats: Default::default(),
            heartrate_stats: Default::default(),
        }
    }

    fn get_available_data_ids(&self) -> Vec<&'static str> {
        [
            ("speed", self.has_speed_data()),
            ("heartrate", self.has_heartrate_data()),
            ("altitude", self.has_altitude_data()),
            ("temperature", self.has_temperature_data()),
        ]
        .into_iter()
        .filter_map(|(data_id, has_data)| has_data.then_some(data_id))
        .collect()
    }

    fn has_temperature_data(&self) -> bool {
        self.temp_stats.no_records > 0
    }

    fn has_altitude_data(&self) -> bool {
        self.altitude_stats.no_records > 0
    }

    fn has_speed_data(&self) -> bool {
        self.speed_stats.no_records > 0
    }

    fn has_heartrate_data(&self) -> bool {
        self.heartrate_stats.no_records > 0
    }
}

fn parse_fit_file(file_path: &PathBuf) -> Result<Activity> {
    use fitparser::profile::MesgNum;
    use std::io::BufReader;

    let path = std::path::Path::new(file_path);
    // Open and parse FIT file
    let fp = fs::File::open(path)
        .with_context(|| format!("Failed to open file: {}", file_path.display()))?;

    let mut reader = BufReader::new(fp);
    let decode_opts = HashSet::from_iter([
        DecodeOption::DropUnknownFields,
        DecodeOption::DropUnknownMessages,
    ]);

    let data_records = fitparser::de::from_reader_with_options(&mut reader, &decode_opts)
        .with_context(|| format!("Failed to parse FIT file: {}", file_path.display()))?;

    // Create activity ID from filename
    let id = path
        .file_name()
        .unwrap()
        .to_str()
        .unwrap_or("unknown")
        .replace(' ', "_");

    let mut activity = Activity::new(id);

    // Sum variables for calculating averages
    let mut sum_temperature: i64 = 0;
    let mut sum_heartrate: u64 = 0;
    let mut sum_speed: f64 = 0.0;
    let mut sum_altitude: f64 = 0.0;

    // Parse all records
    for record in data_records {
        match record.kind() {
            MesgNum::Session => {
                for field in record.into_vec() {
                    let field_name = field.name();
                    // println!("session field: {} {:?}", field_name, field.value());

                    match field_name {
                        "sport" => {
                            if let fitparser::Value::String(sport) = field.value() {
                                activity.activity_type = Some(sport.clone());
                            }
                        }
                        "start_time" => {
                            if let fitparser::Value::Timestamp(v) = field.value() {
                                activity.time_stats.start = Some(*v);
                            }
                        }
                        "total_elapsed_time" => {
                            if let fitparser::Value::Float64(v) = field.value() {
                                let timer_time = Duration::from_secs_f64(*v);
                                activity.time_stats.total = Some(timer_time);
                            }
                        }
                        "total_timer_time" => {
                            if let fitparser::Value::Float64(v) = field.value() {
                                let timer_time = Duration::from_secs_f64(*v);
                                if let Some(total) = activity.time_stats.total {
                                    activity.time_stats.pause = Some(total - timer_time);
                                }
                            }
                        }
                        "total_distance" => {
                            if let fitparser::Value::Float64(v) = field.value() {
                                let d = Distance(*v);
                                activity.total_distance = Some(d);
                            }
                        }
                        _ => {
                            // Print other fields for debugging
                            // println!("  {}: {:?}", field_name, field.value());
                        }
                    }
                }
            }
            MesgNum::Record => {
                let mut rec = Record::default();

                for field in record.into_vec() {
                    let field_name = field.name();

                    match field_name {
                        "timestamp" => {
                            if let fitparser::Value::Timestamp(v) = field.value() {
                                rec.timestamp = *v;
                            }
                        }
                        "position_lat" => {
                            if let fitparser::Value::SInt32(v) = field.value() {
                                let value = LatLong::from_semicircle(*v);
                                rec.position_lat = Some(value);
                            }
                        }
                        "position_long" => {
                            if let fitparser::Value::SInt32(v) = field.value() {
                                let value = LatLong::from_semicircle(*v);
                                rec.position_long = Some(value);
                            }
                        }
                        "altitude" | "enhanced_altitude" => {
                            if rec.altitude.is_none()
                                && let fitparser::Value::Float64(v) = field.value()
                            {
                                activity.altitude_stats.no_records += 1;

                                let value = Altitude(*v);
                                rec.altitude = Some(value);

                                activity.altitude_stats.min = Some(
                                    activity
                                        .altitude_stats
                                        .min
                                        .map_or(value, |prev| prev.min(value)),
                                );

                                activity.altitude_stats.max = Some(
                                    activity
                                        .altitude_stats
                                        .max
                                        .map_or(value, |prev| prev.max(value)),
                                );

                                sum_altitude += value.0;
                            }
                        }
                        "heart_rate" => {
                            if let fitparser::Value::UInt8(v) = field.value() {
                                activity.heartrate_stats.no_records += 1;

                                let value = Heartrate(*v);
                                rec.heartrate = Some(value);

                                activity.heartrate_stats.min = Some(
                                    activity
                                        .heartrate_stats
                                        .min
                                        .map_or(value, |prev| prev.min(value)),
                                );

                                activity.heartrate_stats.max = Some(
                                    activity
                                        .heartrate_stats
                                        .max
                                        .map_or(value, |prev| prev.max(value)),
                                );

                                sum_heartrate += value.0 as u64;
                            }
                        }
                        "distance" => {
                            if let fitparser::Value::Float64(v) = field.value() {
                                let d = Distance::from_unscaled(*v);
                                rec.distance = Some(d);
                            }
                        }
                        "speed" | "enhanced_speed" => {
                            if rec.speed.is_none()
                                && let fitparser::Value::Float64(v) = field.value()
                            {
                                activity.speed_stats.no_records += 1;

                                let value = Speed(*v);
                                rec.speed = Some(value);

                                activity.speed_stats.min = Some(
                                    activity
                                        .speed_stats
                                        .min
                                        .map_or(value, |prev| prev.min(value)),
                                );

                                activity.speed_stats.max = Some(
                                    activity
                                        .speed_stats
                                        .max
                                        .map_or(value, |prev| prev.max(value)),
                                );

                                sum_speed += value.0;
                            }
                        }
                        "temperature" => {
                            if let fitparser::Value::SInt8(v) = field.value() {
                                activity.temp_stats.no_records += 1;

                                let value = Temperature(*v);
                                rec.temperature = Some(value);

                                activity.temp_stats.min = Some(
                                    activity
                                        .temp_stats
                                        .min
                                        .map_or(value, |prev| prev.min(value)),
                                );

                                activity.temp_stats.max = Some(
                                    activity
                                        .temp_stats
                                        .max
                                        .map_or(value, |prev| prev.max(value)),
                                );

                                sum_temperature += value.0 as i64;
                            }
                        }
                        _ => {}
                    }
                }
                activity.records.push(rec);
            }
            _ => {}
        }
    }

    // Calculate average values.
    // Only if we have accumulated record data before.
    if activity.has_temperature_data() {
        let value = Temperature((sum_temperature / activity.temp_stats.no_records as i64) as i8);
        activity.temp_stats.avg = Some(value);
    }

    if activity.has_heartrate_data() {
        let value = Heartrate((sum_heartrate / activity.heartrate_stats.no_records as u64) as u8);
        activity.heartrate_stats.avg = Some(value);
    }

    if activity.has_speed_data() {
        let value = Speed(sum_speed / activity.speed_stats.no_records as f64);
        activity.speed_stats.avg = Some(value);
    }

    if activity.has_altitude_data() {
        let value = Altitude(sum_altitude / activity.altitude_stats.no_records as f64);
        activity.altitude_stats.avg = Some(value);
    }

    Ok(activity)
}

fn blueprint_default(act: &Activity, map_provider: MapProvider) -> Blueprint {
    let id = &act.id;
    let data_ids = act.get_available_data_ids();
    let time_series_contents: Vec<String> = data_ids
        .iter()
        .map(|data_id| format!("{id}/{data_id}"))
        .collect();

    let container = Horizontal::new(vec![
        MapView::new("map")
            .with_origin(format!("{id}/route"))
            .with_map_provider(map_provider)
            .into(),
        Vertical::new(vec![
            TimeSeriesView::new("metrics")
                .with_origin("{id}")
                .with_contents(time_series_contents)
                .into(),
            TextDocumentView::new("info")
                .with_origin(format!("{id}/info"))
                .into(),
        ])
        .into(),
    ]);

    Blueprint::new(container)
}

fn blueprint_vertical(act: &Activity, map_provider: MapProvider) -> Blueprint {
    let id = &act.id;
    let data_ids = act.get_available_data_ids();

    // Build the horizontal row: info text + individual time series views
    let mut info_and_metrics: Vec<ContainerLike> = vec![
        TextDocumentView::new("info")
            .with_origin(format!("{id}/info"))
            .into(),
    ];

    for data_id in data_ids {
        info_and_metrics.push(
            TimeSeriesView::new(data_id.to_string())
                .with_origin(format!("{id}/{data_id}"))
                .into(),
        );
    }

    let container = Vertical::new(vec![
        MapView::new("map")
            .with_origin(format!("{id}/route"))
            .with_map_provider(map_provider)
            .into(),
        Horizontal::new(info_and_metrics).into(),
    ])
    .with_row_shares([3.0, 1.0]); // Map gets 75% height, info+metrics get 25%

    Blueprint::new(container)
        .with_blueprint_panel(BlueprintPanel::from_state("collapsed"))
        .with_selection_panel(SelectionPanel::from_state("collapsed"))
        .with_time_panel(TimePanel::new().with_state_str("collapsed"))
}

fn main() -> Result<()> {
    // env
    dotenvy::dotenv().ok();

    let args = Args::parse();

    // Validate file extension
    if args.fit.extension().and_then(|s| s.to_str()) != Some("fit") {
        anyhow::bail!(
            "Error: File must have .fit extension, got '{}'",
            args.fit.display()
        );
    }

    // Validate file exists
    if !args.fit.exists() {
        anyhow::bail!("Error: File does not exist: '{}'", args.fit.display());
    }

    let use_mapbox = std::env::var("RERUN_MAPBOX_ACCESS_TOKEN").is_ok();
    let map_provider = match args.map {
        MapStyle::Dark if use_mapbox => MapProvider::MapboxDark,
        MapStyle::Light if use_mapbox => MapProvider::MapboxLight,
        MapStyle::Streets if use_mapbox => MapProvider::MapboxStreets,
        MapStyle::Satellite if use_mapbox => MapProvider::MapboxSatellite,
        _ => MapProvider::OpenStreetMap,
    };

    // parse data
    let activity = parse_fit_file(&args.fit)?;

    // Send blueprint based on CLI argument
    let (rec, _) = match args.blueprint {
        BlueprintChoice::Default => {
            let blueprint = blueprint_default(&activity, map_provider);
            args.rerun
                .init_with_blueprint("fit_activities_rerun_rs", blueprint)?
        }
        BlueprintChoice::Vertical => {
            let blueprint = blueprint_vertical(&activity, map_provider);
            args.rerun
                .init_with_blueprint("fit_activities_rerun_rs", blueprint)?
        }
        BlueprintChoice::None => args.rerun.init("fit_activities_rerun_rs")?,
    };

    run(&rec, &activity)
}

fn run(rec: &RecordingStream, act: &Activity) -> anyhow::Result<()> {
    let id = &act.id;
    let data_ids = act.get_available_data_ids();

    // Style scalar lines + max + min points
    // Note: 7F -> 127 for transparency (50%)
    for data_id in data_ids {
        // lines
        rec.log_static(
            format!("{id}/{data_id}/value"),
            &SeriesLines::new().with_names(["values"]).with_widths([2.0]),
        )?;

        // avg
        rec.log_static(
            format!("{id}/{data_id}/avg"),
            &SeriesLines::new()
                .with_names(["avg"])
                .with_colors([Color::from(0x1B7EF77F)])
                .with_widths([2.0]),
        )?;

        // max
        rec.log_static(
            format!("{id}/{data_id}/max"),
            &SeriesPoints::new()
                .with_names(["max"])
                .with_colors([Color::from(0xE11D487F)])
                .with_markers([MarkerShape::Circle])
                .with_marker_sizes([2.0]),
        )?;

        // min
        rec.log_static(
            format!("{id}/{data_id}/min"),
            &SeriesPoints::new()
                .with_names(["min"])
                .with_colors([Color::from(0xFDE0477F)])
                .with_markers([MarkerShape::Circle])
                .with_marker_sizes([2.0]),
        )?;
    }

    // Build info markdown - header section
    let activity_type = act
        .activity_type
        .as_deref()
        .unwrap_or("Summary")
        .to_uppercase();
    let mut info_md = format!(
        "### {activity_type}
- ID **{}**
- NO. RECORDS **{}**

###### **SESSION SUMMARY**",
        act.id,
        act.records.len()
    );

    // --
    // START-TIME
    // --
    if let Some(start_time) = act.time_stats.start {
        info_md.push_str(&format!(
            "\n- START **{}**",
            start_time.format("%d.%m.%Y %H:%M:%S")
        ));
    }

    // --
    // DURATION
    // --
    let mut duration_parts = Vec::new();

    if let Some(total) = &act.time_stats.total {
        duration_parts.push(format!("**{}** (total)", format_time(total)));
    }

    if let Some(pause) = act.time_stats.pause.filter(|&p| p > Duration::ZERO) {
        duration_parts.push(format!("**{}** (pause)", format_time(&pause)));
    }

    if !duration_parts.is_empty() {
        info_md.push_str(&format!("\n- DURATION {}", duration_parts.join(" ")));
    }

    // --
    // DISTANCE
    // --
    if let Some(distance) = &act.total_distance {
        info_md.push_str(&format!("\n- DISTANCE **{}**", distance.format()));
    }

    // --
    // TABLE: header
    // --
    info_md.push_str("\n\n###### **RECORDS SUMMARY**");
    info_md.push_str("\n| | max | min | avg | no. rec.");
    info_md.push_str("\n| --- | --- | --- | --- | --- |");

    // Helper to format optional stat value
    let fmt_stat = |opt: Option<String>| {
        opt.map(|v| format!("**{v}**|"))
            .unwrap_or_else(|| "-- |".to_string())
    };

    // --
    // TABLE: Speed
    // --
    if act.has_speed_data() {
        let s = &act.speed_stats;
        info_md.push_str(&format!(
            "\n|SPEED|{}{}{}**{}**|",
            fmt_stat(s.max.map(|v| v.format())),
            fmt_stat(s.min.map(|v| v.format())),
            fmt_stat(s.avg.map(|v| v.format())),
            s.no_records
        ));
    }

    // --
    // TABLE: Heartrate
    // --
    if act.has_heartrate_data() {
        let h = &act.heartrate_stats;
        info_md.push_str(&format!(
            "\n|♥ RATE|{}{}{}**{}**|",
            fmt_stat(h.max.map(|v| v.format())),
            fmt_stat(h.min.map(|v| v.format())),
            fmt_stat(h.avg.map(|v| v.format())),
            h.no_records
        ));
    }

    // --
    // TABLE: Altitude
    // --
    if act.has_altitude_data() {
        let a = &act.altitude_stats;
        info_md.push_str(&format!(
            "\n|ALTITUDE|{}{}{}**{}**|",
            fmt_stat(a.max.map(|v| v.format())),
            fmt_stat(a.min.map(|v| v.format())),
            fmt_stat(a.avg.map(|v| v.format())),
            a.no_records
        ));
    }

    // --
    // TABLE: Temperature
    // --
    if act.has_temperature_data() {
        let t = &act.temp_stats;
        info_md.push_str(&format!(
            "\n|TEMPERATURE|{}{}{}**{}**|",
            fmt_stat(t.max.map(|v| v.format())),
            fmt_stat(t.min.map(|v| v.format())),
            fmt_stat(t.avg.map(|v| v.format())),
            t.no_records
        ));
    }

    rec.log_static(
        format!("{id}/info"),
        &TextDocument::new(info_md).with_media_type(MediaType::markdown()),
    )?;

    // Collect geo positions from records
    let positions: Vec<(f64, f64)> = act
        .records
        .iter()
        .filter_map(|record| match (record.position_lat, record.position_long) {
            (Some(lat), Some(lon)) => Some((lat.0, lon.0)),
            _ => None,
        })
        .collect();

    if let Some(&first) = positions.first() {
        // Start point
        rec.log_static(
            format!("{id}/route/all/start"),
            &GeoPoints::from_lat_lon([first])
                .with_radii([Radius::new_ui_points(6.0)])
                .with_colors([Color::from(0xF79311FF)]),
        )?;

        // All route
        rec.log_static(
            format!("{id}/route/all"),
            &GeoLineStrings::from_lat_lon([positions.clone()])
                .with_radii([Radius::new_ui_points(2.0)])
                .with_colors([Color::from(0xF793117F)]),
        )?;

        // Finish point
        if let Some(&last) = positions.last() {
            rec.log_static(
                format!("{id}/route/all/finish"),
                &GeoPoints::from_lat_lon([last])
                    .with_radii([Radius::new_ui_points(6.0)])
                    .with_colors([Color::from(0xF793117F)]),
            )?;
        }
    }

    // Log route and position for each record with timestamp
    let mut route_positions = Vec::new();
    for record in &act.records {
        rec.set_timestamp_nanos_since_epoch(
            "timestamp",
            record.timestamp.timestamp_nanos_opt().unwrap_or(0),
        );

        if let (Some(lat), Some(lon)) = (record.position_lat, record.position_long) {
            let pos = (lat.0, lon.0);
            route_positions.push(pos);

            // Log route of current record
            rec.log(
                format!("{id}/route/current"),
                &GeoLineStrings::from_lat_lon([route_positions.clone()])
                    .with_radii([Radius::new_ui_points(2.0)])
                    .with_colors([Color::from(0xF79311FF)]),
            )?;

            // Log point of current record
            rec.log(
                format!("{id}/route/current/location"),
                &GeoPoints::from_lat_lon([pos])
                    .with_radii([Radius::new_ui_points(6.0)])
                    .with_colors([Color::from(0xF79311FF)]),
            )?;
        }

        // Log record data: value / max / avg
        if let Some(speed) = record.speed {
            rec.log(format!("{id}/speed"), &Scalars::new([speed.0]))?;
            if act.speed_stats.max.is_some_and(|m| m.0 == speed.0) {
                rec.log(format!("{id}/speed/max"), &Scalars::new([speed.0]))?;
            }
            if act.speed_stats.min.is_some_and(|m| m.0 == speed.0) {
                rec.log(format!("{id}/speed/min"), &Scalars::new([speed.0]))?;
            }
            if let Some(avg) = act.speed_stats.avg {
                rec.log(format!("{id}/speed/avg"), &Scalars::new([avg.0]))?;
            }
        }

        if let Some(heartrate) = record.heartrate {
            let value = heartrate.0 as f64;
            rec.log(format!("{id}/heartrate"), &Scalars::new([value]))?;
            if act.heartrate_stats.max.is_some_and(|m| m == heartrate) {
                rec.log(format!("{id}/heartrate/max"), &Scalars::new([value]))?;
            }
            if act.heartrate_stats.min.is_some_and(|m| m == heartrate) {
                rec.log(format!("{id}/heartrate/min"), &Scalars::new([value]))?;
            }
            if let Some(avg) = act.heartrate_stats.avg {
                rec.log(format!("{id}/heartrate/avg"), &Scalars::new([avg.0 as f64]))?;
            }
        }

        if let Some(altitude) = record.altitude {
            let value = altitude.0;
            rec.log(format!("{id}/altitude"), &Scalars::new([value]))?;
            if act.altitude_stats.max.is_some_and(|m| m == altitude) {
                rec.log(format!("{id}/altitude/max"), &Scalars::new([value]))?;
            }
            if act.altitude_stats.min.is_some_and(|m| m == altitude) {
                rec.log(format!("{id}/altitude/min"), &Scalars::new([value]))?;
            }
            if let Some(avg) = act.altitude_stats.avg {
                rec.log(format!("{id}/altitude/avg"), &Scalars::new([avg.0]))?;
            }
        }

        if let Some(temperature) = record.temperature {
            let value = temperature.0 as f64;
            rec.log(format!("{id}/temperature"), &Scalars::new([value]))?;
            if act.temp_stats.max.is_some_and(|m| m == temperature) {
                rec.log(format!("{id}/temperature/max"), &Scalars::new([value]))?;
            }
            if act.temp_stats.min.is_some_and(|m| m == temperature) {
                rec.log(format!("{id}/temperature/min"), &Scalars::new([value]))?;
            }
            if let Some(avg) = act.temp_stats.avg {
                rec.log(
                    format!("{id}/temperature/avg"),
                    &Scalars::new([avg.0 as f64]),
                )?;
            }
        }
    }

    Ok(())
}
