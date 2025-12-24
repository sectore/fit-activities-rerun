use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use clap::Parser;
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
    position_lat: Option<f32>,
    position_long: Option<f32>,
    distance: Option<u32>,
    speed: Option<u16>,
    heartrate: Option<u8>,
    temperature: Option<i8>,
    altitude: Option<u16>,
}

#[derive(Debug, Default)]
struct TemperatureStats {
    no_records: usize,
    max: Option<i8>,
    min: Option<i8>,
    avg: Option<i8>,
}

#[derive(Debug, Default)]
struct AltitudeStats {
    no_records: usize,
    max: Option<u16>,
    min: Option<u16>,
    avg: Option<u16>,
}

#[derive(Debug, Default)]
struct SpeedStats {
    no_records: usize,
    max: Option<u16>,
    min: Option<u16>,
    avg: Option<u16>,
}

#[derive(Debug, Default)]
struct HeartrateStats {
    no_records: usize,
    max: Option<u8>,
    min: Option<u8>,
    avg: Option<u8>,
}

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

fn parse_fit_file(file_path: &PathBuf) -> Result<Activity> {
    use fit_rust::Fit;
    use fit_rust::protocol::FitMessage;
    use fit_rust::protocol::message_type::MessageType;

    // Read and parse FIT file
    let data = fs::read(file_path)
        .with_context(|| format!("Failed to read file: {}", file_path.display()))?;

    let fit = Fit::read(data)
        .with_context(|| format!("Failed to parse FIT file: {}", file_path.display()))?;

    // Create activity ID from filename
    let id = file_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .replace(' ', "_");

    let mut activity = Activity::new(id);

    // Record-based fallback tracking (only used if session doesn't provide values)
    let mut record_min_temperature: Option<i8> = None;
    let mut record_max_temperature: Option<i8> = None;
    let mut record_max_heartrate: Option<u8> = None;
    let mut record_max_speed: Option<u16> = None;
    let mut record_min_altitude: Option<u16> = None;
    let mut record_max_altitude: Option<u16> = None;

    // Sum variables for calculating averages
    let mut sum_temperature: i64 = 0;
    let mut sum_heartrate: u64 = 0;
    let mut sum_speed: u64 = 0;
    let mut sum_altitude: u64 = 0;

    println!("=== Parsing FIT file ===");
    println!("Total messages: {}", fit.data.len());

    // Parse Session messages
    for message in &fit.data {
        match message {
            FitMessage::Data(msg) if msg.data.message_type == MessageType::Session => {
                println!("\n--- Parsing Session Data ---");

                for field in &msg.data.values {
                    use fit_rust::protocol::value::Value;

                    match field.field_num {
                        // sport
                        5 => {
                            println!("  Field 5 (sport): {:?}", field.value);
                            if let Value::Enum(name) = &field.value {
                                activity.activity_type = Some(name.to_string());
                            }
                        }
                        // total_elapsed_time
                        7 => {
                            println!("  Field 7 (total_elapsed_time): {:?}", field.value);
                            if let Value::U32(v) = &field.value {
                                activity.time_stats.total = Some(*v);
                            }
                        }
                        // total_timer_time
                        8 => {
                            println!("  Field 8 (total_timer_time): {:?}", field.value);
                            if let Value::U32(v) = &field.value {
                                let timer_time = *v;
                                if let Some(total) = activity.time_stats.total {
                                    activity.time_stats.pause = Some(total - timer_time);
                                }
                            }
                        }
                        // total_distance
                        9 => {
                            println!("  Field 9 (total_distance): {:?}", field.value);
                            if let Value::U32(v) = &field.value {
                                activity.total_distance = Some(*v as f64);
                            }
                        }
                        // enhanced_avg_speed
                        14 => {
                            if let Value::U16(v) = &field.value {
                                activity.speed_stats.avg = Some(*v);
                            }
                        }
                        // enhanced_max_speed
                        15 => {
                            if let Value::U16(v) = &field.value {
                                activity.speed_stats.max = Some(*v);
                            }
                        }
                        // avg_heart_rate
                        16 => {
                            if let Value::U8(v) = &field.value {
                                activity.heartrate_stats.avg = Some(*v);
                            }
                        }
                        // max_heart_rate
                        17 => {
                            if let Value::U8(v) = &field.value {
                                activity.heartrate_stats.max = Some(*v);
                            }
                        }
                        // NOTE: Session does NOT provide:
                        // - Temperature stats (min, max, avg) - calculated from records
                        // - Altitude stats (min, max, avg) - calculated from records
                        // - min_heart_rate - calculated from records
                        // - min_speed - calculated from records
                        _ => {
                            // Print unknown fields to help identify field numbers
                            println!("  Field {}: {:?}", field.field_num, field.value);
                        }
                    }
                }
            }
            FitMessage::Data(msg) if msg.data.message_type == MessageType::Record => {
                use fit_rust::protocol::value::Value;

                let mut record = Record::default();

                for field in &msg.data.values {
                    match field.field_num {
                        // `timestamp`
                        253 => {
                            if let Value::Time(v) = &field.value {
                                record.timestamp =
                                    DateTime::from_timestamp(*v as i64, 0).unwrap_or_else(Utc::now);
                            }
                        }
                        // `position_lat`
                        0 => {
                            if let Value::F32(v) = &field.value {
                                record.position_lat = Some(*v);
                            }
                        }
                        // `position_long`
                        1 => {
                            if let Value::F32(v) = &field.value {
                                record.position_long = Some(*v);
                            }
                        }
                        // altitude
                        2 => {
                            if let Value::U16(v) = &field.value {
                                activity.altitude_stats.no_records += 1;
                                let value = *v;
                                record.altitude = Some(value);

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
                        // heart_rate (bpm)
                        3 => {
                            if let Value::U8(v) = &field.value {
                                activity.heartrate_stats.no_records += 1;
                                let value = *v;
                                record.heartrate = Some(value);

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
                        // distance
                        5 => {
                            if let Value::U32(v) = &field.value {
                                record.distance = Some(*v);
                            }
                        }
                        // speed
                        6 => {
                            if let Value::U16(v) = &field.value {
                                activity.speed_stats.no_records += 1;
                                let value = *v;
                                record.speed = Some(value);

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
                                    sum_speed += value as u64;
                                }
                            }
                        }
                        // temperature
                        13 => {
                            if let Value::I8(v) = &field.value {
                                activity.temp_stats.no_records += 1;
                                let value = *v;
                                record.temperature = Some(value);

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

                activity.records.push(record);
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

    if sum_speed > 0 {
        activity.speed_stats.avg =
            Some((sum_speed / activity.speed_stats.no_records as u64) as u16);
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
    println!("Speed: {:?}", activity.speed_stats);
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
        println!(
            "Distance: {} cm ({:.2} m) ({:.2} km)",
            distance,
            distance / 100.0,
            distance / 100_000.0
        );
    }
    println!("Total records: {}", activity.records.len());
    println!("Temperature records: {}", activity.temp_stats.no_records);
    println!("Altitude records: {}", activity.altitude_stats.no_records);
    println!("Heartrate records: {}", activity.heartrate_stats.no_records);
    println!("Speed records: {}", activity.speed_stats.no_records);

    Ok(())
}
