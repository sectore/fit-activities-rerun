use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use clap::Parser;
use fitparser::de::DecodeOption;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "fit-activities-rerun",
    about = "Visualize `*.fit` data using Rerun.",
    version
)]
struct Args {
    /// Path to the .fit file
    #[arg(long, value_name = "FILE")]
    fit: PathBuf,
}

#[derive(Debug, Default)]
struct TimeStats {
    start: Option<DateTime<Utc>>,
    total: Option<u32>,
    pause: Option<u32>,
}

#[derive(Debug, Default)]
struct Record {
    timestamp: DateTime<Utc>,
    position_lat: Option<f64>,
    position_long: Option<f64>,
    distance: Option<u32>,
    speed: Option<f64>,
    heartrate: Option<u8>,
    temperature: Option<i8>,
    altitude: Option<u16>,
}

#[derive(Debug, Default)]
struct Stats<T> {
    no_records: usize,
    max: Option<T>,
    min: Option<T>,
    avg: Option<T>,
}

// Type aliases for domain-specific stats
type TemperatureStats = Stats<i8>;
type AltitudeStats = Stats<u16>;
type SpeedStats = Stats<f64>;
type HeartrateStats = Stats<u8>;

#[derive(Debug)]
struct Activity {
    id: String,
    records: Vec<Record>,
    activity_type: Option<String>,
    time_stats: TimeStats,
    total_distance: Option<f64>,
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

// Conversation formulas:
// degrees = semicircles × (180 / 2^31)
// semicircles = degrees * ( 2^31 / 180 )
// @see https://learn.microsoft.com/en-us/previous-versions/windows/embedded/cc510650(v=msdn.10)
const SEMICIRCLES_TO_DEGREES: f64 = 180.0 / (1_u64 << 31) as f64;

fn semicircle_to_degree(semicircle: i32) -> f64 {
    (f64::from(semicircle) * SEMICIRCLES_TO_DEGREES)
}

fn format_distance(meters: f64) -> String {
    if meters < 1000.0 {
        format!("{}m", meters as u32)
    } else {
        format!("{:.1}km", meters / 1000.0)
    }
}

fn to_km_per_hour(meters_per_second: f64) -> f64 {
    meters_per_second * 3.6
}

fn format_speed(meters_per_second: f64) -> String {
    format!("{:.2} km/h", to_km_per_hour(meters_per_second)).replace(".00", "")
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

    // Record-based fallback tracking (only used if session doesn't provide values)
    let mut record_min_temperature: Option<i8> = None;
    let mut record_max_temperature: Option<i8> = None;
    let mut record_max_heartrate: Option<u8> = None;
    let mut record_max_speed: Option<f64> = None;
    let mut record_min_altitude: Option<u16> = None;
    let mut record_max_altitude: Option<u16> = None;

    // Sum variables for calculating averages
    let mut sum_temperature: i64 = 0;
    let mut sum_heartrate: u64 = 0;
    let mut sum_speed: f64 = 0.0;
    let mut sum_altitude: u64 = 0;

    println!("=== Parsing FIT file ===");
    println!("Total records: {}", data_records.len());

    let mut idx = 0;

    // Parse all records
    for record in data_records {
        match record.kind() {
            MesgNum::Session => {
                println!("\n--- Parsing Session Data ---");

                for field in record.into_vec() {
                    let field_name = field.name();
                    println!("session field: {} {:?}", field_name, field.value());

                    match field_name {
                        "sport" => {
                            if let fitparser::Value::String(sport) = field.value() {
                                activity.activity_type = Some(sport.clone());
                            }
                        }
                        "total_elapsed_time" => {
                            if let fitparser::Value::Float64(v) = field.value() {
                                activity.time_stats.total = Some((v * 1000.0) as u32);
                            }
                        }
                        "total_timer_time" => {
                            if let fitparser::Value::Float64(v) = field.value() {
                                let timer_time = (v * 1000.0) as u32;
                                if let Some(total) = activity.time_stats.total {
                                    activity.time_stats.pause = Some(total - timer_time);
                                }
                            }
                        }
                        "total_distance" => {
                            if let fitparser::Value::Float64(v) = field.value() {
                                activity.total_distance = Some(*v);
                            }
                        }
                        "enhanced_avg_speed" | "avg_speed" => {
                            if let fitparser::Value::Float64(v) = field.value() {
                                activity.speed_stats.avg = Some(*v);
                            }
                        }
                        "enhanced_max_speed" | "max_speed" => {
                            if let fitparser::Value::Float64(v) = field.value() {
                                activity.speed_stats.max = Some(*v);
                            }
                        }
                        "avg_heart_rate" => {
                            if let fitparser::Value::UInt8(v) = field.value() {
                                activity.heartrate_stats.avg = Some(*v);
                            }
                        }
                        "max_heart_rate" => {
                            if let fitparser::Value::UInt8(v) = field.value() {
                                activity.heartrate_stats.max = Some(*v);
                            }
                        }
                        _ => {
                            // Print all fields for debugging
                            println!("  {}: {:?}", field_name, field.value());
                        }
                    }
                }
            }
            MesgNum::Record => {
                let mut rec = Record::default();

                for field in record.into_vec() {
                    let field_name = field.name();
                    if idx == 11 {
                        println!("idx: {}", idx);
                        println!("record field: {} {:?}", field_name, field.value());
                    }

                    match field_name {
                        "timestamp" => {
                            if let fitparser::Value::Timestamp(ts) = field.value() {
                                rec.timestamp = ts.with_timezone(&Utc);
                            }
                        }
                        "position_lat" => {
                            if let fitparser::Value::SInt32(v) = field.value() {
                                let value = semicircle_to_degree(*v);
                                rec.position_lat = Some(value);
                            }
                        }
                        "position_long" => {
                            if let fitparser::Value::SInt32(v) = field.value() {
                                let value = semicircle_to_degree(*v);
                                rec.position_long = Some(value);
                            }
                        }
                        "altitude" | "enhanced_altitude" => {
                            if let fitparser::Value::Float64(v) = field.value() {
                                activity.altitude_stats.no_records += 1;
                                let value = (*v * 5.0 + 500.0) as u16;
                                rec.altitude = Some(value);

                                // Only calculate from records if session didn't provide `min`
                                if activity.altitude_stats.min.is_none() {
                                    record_min_altitude = Some(
                                        record_min_altitude.map_or(value, |min| min.min(value)),
                                    );
                                }

                                // Only calculate from records if session didn't provide `max`
                                if activity.altitude_stats.max.is_none() {
                                    record_max_altitude = Some(
                                        record_max_altitude.map_or(value, |max| max.max(value)),
                                    );
                                }

                                // Only accumulate for `avg` if session didn't provide it
                                if activity.altitude_stats.avg.is_none() {
                                    sum_altitude += value as u64;
                                }
                            }
                        }
                        "heart_rate" => {
                            if let fitparser::Value::UInt8(v) = field.value() {
                                activity.heartrate_stats.no_records += 1;
                                let value = *v;
                                rec.heartrate = Some(value);

                                // Always calculate `min` (session never provides it)
                                activity.heartrate_stats.min = Some(
                                    activity
                                        .heartrate_stats
                                        .min
                                        .map_or(value, |min| min.min(value)),
                                );

                                // Only calculate from records if session didn't provide `max`
                                if activity.heartrate_stats.max.is_none() {
                                    record_max_heartrate = Some(
                                        record_max_heartrate.map_or(value, |max| max.max(value)),
                                    );
                                }

                                // Only accumulate for `avg` if session didn't provide it
                                if activity.heartrate_stats.avg.is_none() {
                                    sum_heartrate += value as u64;
                                }
                            }
                        }
                        "distance" => {
                            if let fitparser::Value::Float64(v) = field.value() {
                                rec.distance = Some((v * 100.0) as u32);
                            }
                        }
                        "speed" | "enhanced_speed" => {
                            if let fitparser::Value::Float64(v) = field.value() {
                                activity.speed_stats.no_records += 1;
                                let value = *v;
                                rec.speed = Some(value);

                                // Always calculate `min` (session never provides it)
                                activity.speed_stats.min = Some(
                                    activity.speed_stats.min.map_or(value, |min| min.min(value)),
                                );

                                // Only calculate from records if session didn't provide `max`
                                if activity.speed_stats.max.is_none() {
                                    record_max_speed =
                                        Some(record_max_speed.map_or(value, |max| max.max(value)));
                                }

                                // Only accumulate for `avg` if session didn't provide it
                                if activity.speed_stats.avg.is_none() {
                                    sum_speed += value;
                                }
                            }
                        }
                        "temperature" => {
                            if let fitparser::Value::SInt8(v) = field.value() {
                                activity.temp_stats.no_records += 1;
                                let value = *v;
                                rec.temperature = Some(value);

                                // Only calculate from records if session didn't provide `min`
                                if activity.temp_stats.min.is_none() {
                                    record_min_temperature = Some(
                                        record_min_temperature.map_or(value, |min| min.min(value)),
                                    );
                                }

                                // Only calculate from records if session didn't provide `max`
                                if activity.temp_stats.max.is_none() {
                                    record_max_temperature = Some(
                                        record_max_temperature.map_or(value, |max| max.max(value)),
                                    );
                                }

                                // Only accumulate for `avg` if session didn't provide it
                                if activity.temp_stats.avg.is_none() {
                                    sum_temperature += value as i64;
                                }
                            }
                        }
                        _ => {}
                    }
                }

                activity.records.push(rec);
                idx += 1;
            }
            _ => {}
        }
    }

    // Assign record-based calculations if session didn't provide them
    if let Some(min) = record_min_temperature {
        activity.temp_stats.min = Some(min);
    }
    if let Some(max) = record_max_temperature {
        activity.temp_stats.max = Some(max);
    }

    if let Some(max) = record_max_heartrate {
        activity.heartrate_stats.max = Some(max);
    }

    if let Some(max) = record_max_speed {
        activity.speed_stats.max = Some(max);
    }

    if let Some(min) = record_min_altitude {
        activity.altitude_stats.min = Some(min);
    }
    if let Some(max) = record_max_altitude {
        activity.altitude_stats.max = Some(max);
    }

    // Calculate average values.
    // Only if we have accumulated data by counting `sum_` values before.
    if sum_temperature != 0 {
        activity.temp_stats.avg =
            Some((sum_temperature / activity.temp_stats.no_records as i64) as i8);
    }

    if sum_heartrate > 0 {
        activity.heartrate_stats.avg =
            Some((sum_heartrate / activity.heartrate_stats.no_records as u64) as u8);
    }

    if sum_speed > 0.0 {
        activity.speed_stats.avg = Some(sum_speed / activity.speed_stats.no_records as f64);
    }

    if sum_altitude > 0 {
        activity.altitude_stats.avg =
            Some((sum_altitude / activity.altitude_stats.no_records as u64) as u16);
    }

    println!("\n--- Parsing Complete ---");
    println!("Total records parsed: {}", activity.records.len());

    // Print stats
    println!("\n--- Stats Summary ---");
    println!("Temperature: {:?}", activity.temp_stats);
    println!("Heartrate: {:?}", activity.heartrate_stats);
    if let Some(min) = activity.speed_stats.min {
        println!("Speed min: {}", format_speed(min));
    }
    if let Some(max) = activity.speed_stats.max {
        println!("Speed max: {}", format_speed(max));
    }
    if let Some(avg) = activity.speed_stats.avg {
        println!("Speed avg: {}", format_speed(avg));
    }
    println!("Altitude: {:?}", activity.altitude_stats);

    // Print first 3 records as samples
    if !activity.records.is_empty() {
        println!("\nSample records (first 3):");
        for (i, record) in activity.records.iter().take(3).enumerate() {
            println!("  Record #{}: {:?}", i, record);
        }
    }

    Ok(activity)
}

fn main() -> Result<()> {
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

    // Parse FIT file
    let activity = parse_fit_file(&args.fit)?;

    println!("\n=== Activity Summary ===");
    println!("Activity ID: {}", activity.id);
    if let Some(activity_type) = &activity.activity_type {
        println!("Type: {}", activity_type);
    }
    if let Some(time) = activity.time_stats.total {
        println!("Total time: {} ms", time);
    }
    if let Some(pause) = activity.time_stats.pause {
        println!("Pause time: {} ms", pause);
    }
    if let Some(distance) = activity.total_distance {
        println!("Distance: {})", format_distance(distance));
    }
    println!("Total records: {}", activity.records.len());
    println!("Temperature records: {}", activity.temp_stats.no_records);
    println!("Altitude records: {}", activity.altitude_stats.no_records);
    println!("Heartrate records: {}", activity.heartrate_stats.no_records);
    println!("Speed records: {}", activity.speed_stats.no_records);

    Ok(())
}
