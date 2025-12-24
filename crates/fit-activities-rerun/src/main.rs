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
    heartrate: Option<i32>,
    temperature: Option<i8>,
    altitude: Option<u16>,
}

#[derive(Debug, Default)]
struct TemperatureStats {
    no_records: usize,
    max: Option<i32>,
    min: Option<i32>,
    avg: Option<i32>,
}

#[derive(Debug, Default)]
struct AltitudeStats {
    no_records: usize,
    max: Option<i32>,
    min: Option<i32>,
    avg: Option<i32>,
}

#[derive(Debug, Default)]
struct SpeedStats {
    no_records: usize,
    max: Option<i32>,
    min: Option<i32>,
    avg: Option<i32>,
}

#[derive(Debug, Default)]
struct HeartrateStats {
    no_records: usize,
    max: Option<i32>,
    min: Option<i32>,
    avg: Option<i32>,
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
                        _ => {}
                    }
                }
            }
            FitMessage::Data(msg) if msg.data.message_type == MessageType::Record => {
                use fit_rust::protocol::value::Value;

                let mut record = Record::default();

                for field in &msg.data.values {
                    match field.field_num {
                        // timestamp
                        253 => {
                            if let Value::Time(v) = &field.value {
                                record.timestamp =
                                    DateTime::from_timestamp(*v as i64, 0).unwrap_or_else(Utc::now);
                            }
                        }
                        // position_lat
                        0 => {
                            if let Value::F32(v) = &field.value {
                                record.position_lat = Some(*v);
                            }
                        }
                        // position_long
                        1 => {
                            if let Value::F32(v) = &field.value {
                                record.position_long = Some(*v);
                            }
                        }
                        // altitude
                        2 => {
                            if let Value::U16(v) = &field.value {
                                record.altitude = Some(*v);
                            }
                        }
                        // heart_rate (bpm)
                        3 => {
                            if let Value::U8(v) = &field.value {
                                record.heartrate = Some(*v as i32);
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
                                record.speed = Some(*v);
                            }
                        }
                        // temperature
                        13 => {
                            if let Value::I8(v) = &field.value {
                                record.temperature = Some(*v);
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

    println!("\n--- Parsing Complete ---");
    println!("Total records parsed: {}", activity.records.len());

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

    Ok(())
}
