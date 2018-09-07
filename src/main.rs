/*
 * Time: at the top of every minute
 * RAM/CPU/Temp: every 5s
 * Network: ip monitor
 * Volume: pactl subscribe
 */

extern crate chrono;
extern crate sensors;
extern crate systemstat;

use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time;

use sensors::{FeatureType, Sensors, Subfeature, SubfeatureType};
use systemstat::{Platform, System};

#[derive(Debug)]
struct DisplayFields {
    time: String,
    systemstat: String,
    temp: String,
}

#[derive(Debug)]
struct Concurrency {
    lock: Mutex<DisplayFields>,
    condition: Condvar,
}

fn time_thread(conc: Arc<Concurrency>) {
    loop {
        let new_time = chrono::Local::now()
            .format("📆 %a, %d %h ⸱ 🕓 %R")
            .to_string();
        {
            let mut df = conc.lock.lock().unwrap();
            df.time = new_time;
            conc.condition.notify_one();
        }
        let now = chrono::Local::now();
        let now_ts = now.timestamp_millis();
        let next_ts = ((now_ts / 60_000) + 1) * 60_000;
        let sleep_time = time::Duration::from_millis((next_ts - now_ts) as u64);
        thread::sleep(sleep_time);
    }
}

fn plugged(sys: &System) -> String {
    if let Ok(plugged) = sys.on_ac_power() {
        if plugged {
            "🔌".to_string()
        } else {
            "🔋".to_string()
        }
    } else {
        "🔌".to_string()
    }
}

fn battery(sys: &System) -> String {
    if let Ok(bat) = sys.battery_life() {
        format!("{} {:.1}%", plugged(sys), bat.remaining_capacity * 100.)
    } else {
        "".to_string()
    }
}

fn ram(sys: &System) -> String {
    if let Ok(mem) = sys.memory() {
        let used = mem.total - mem.free;
        format!("▯ {}", used)
    } else {
        "▯ _".to_string()
    }
}

fn cpu(sys: &System) -> String {
    if let Ok(load) = sys.load_average() {
        format!("⚙ {:.2}", load.one)
    } else {
        "⚙ _".to_string()
    }
}

fn systemstat_thread(conc: Arc<Concurrency>) {
    let sys = System::new();
    loop {
        let new_stat = format!("{} ⸱ {} ⸱ {}", battery(&sys), ram(&sys), cpu(&sys)).to_string();
        {
            let mut df = conc.lock.lock().unwrap();
            if df.systemstat != new_stat {
                df.systemstat = new_stat;
                conc.condition.notify_one();
            }
        }
        thread::sleep(time::Duration::from_secs(5));
    }
}

fn find_cpu_temp(sensors: &Sensors) -> Option<Subfeature> {
    for chip in sensors.into_iter() {
        for feature in chip.into_iter() {
            if feature.feature_type() == &FeatureType::SENSORS_FEATURE_TEMP {
                if let Some(subfeature) =
                    feature.get_subfeature(SubfeatureType::SENSORS_SUBFEATURE_TEMP_INPUT)
                {
                    return Some(subfeature);
                }
            }
        }
    }
    return None;
}

fn sensors_thread(conc: Arc<Concurrency>) {
    let sensors = Sensors::new();
    if let Some(temp_sensor) = find_cpu_temp(&sensors) {
        loop {
            let new_temp = format!("\u{1F321} {}", temp_sensor.get_value().unwrap());
            {
                let mut df = conc.lock.lock().unwrap();
                if df.temp != new_temp {
                    df.temp = new_temp;
                    conc.condition.notify_one();
                }
            }
            thread::sleep(time::Duration::from_secs(5));
        }
    }
}

fn display_thread(conc: Arc<Concurrency>) {
    let mut df = conc.lock.lock().unwrap();
    loop {
        println!("{:?}", *df);
        df = conc.condition.wait(df).unwrap();
    }
}

fn main() {
    let conc = Arc::new(Concurrency {
        lock: Mutex::new(DisplayFields {
            time: String::new(),
            systemstat: String::new(),
            temp: String::new(),
        }),
        condition: Condvar::new(),
    });

    {
        let conc2 = Arc::clone(&conc);
        thread::spawn(move || time_thread(conc2));
    }

    {
        let conc2 = Arc::clone(&conc);
        thread::spawn(move || systemstat_thread(conc2));
    }

    {
        let conc2 = Arc::clone(&conc);
        thread::spawn(move || sensors_thread(conc2));
    }

    display_thread(conc);
}
