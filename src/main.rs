/*
 * Time: at the top of every minute
 * RAM/CPU/Temp: every 5s
 * Network: ip monitor
 * Volume: pactl subscribe
 */

#![recursion_limit = "1024"]

#[macro_use]
extern crate error_chain;

use std::collections::HashSet;
use std::fs::File;
use std::io::prelude::*;
use std::io::BufReader;
use std::process::{Command, Stdio};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time;

use chrono;
use lazy_static::lazy_static;
use regex::Regex;
use sensors::{FeatureType, Sensors, Subfeature, SubfeatureType};
use systemstat::{Platform, System};
use xcb;

mod errors {
    error_chain! {}
}

use crate::errors::*;

const POLL_TIME: time::Duration = time::Duration::from_secs(5);

#[derive(Debug)]
struct DisplayFields {
    time: String,
    systemstat: String,
    temp: String,
    net: String,
    volume: String,
}

impl ToString for DisplayFields {
    fn to_string(&self) -> String {
        format!(
            "{}{}{}{}{}",
            self.volume, self.net, self.systemstat, self.temp, self.time
        )
        .to_string()
    }
}

#[derive(Debug)]
struct Concurrency {
    lock: Mutex<DisplayFields>,
    condition: Condvar,
}

fn time_thread(conc: Arc<Concurrency>) {
    loop {
        let new_date = chrono::Local::now().format("📆 %a, %d %h");
        let new_utc_time = chrono::Utc::now().format("%HZ");
        let new_local_time = chrono::Local::now().format("%R");
        let new_time = format!("{} 🕓 {} {}", new_date, new_utc_time, new_local_time).to_string();
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
        format!("{} {:.1}% ⸱ ", plugged(sys), bat.remaining_capacity * 100.)
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
        let new_stat = format!("{}{} ⸱ {} ⸱ ", battery(&sys), ram(&sys), cpu(&sys)).to_string();
        {
            let mut df = conc.lock.lock().unwrap();
            if df.systemstat != new_stat {
                df.systemstat = new_stat;
                conc.condition.notify_one();
            }
        }
        thread::sleep(POLL_TIME);
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
            let new_temp = format!("\u{1F321} {:.1} ⸱ ", temp_sensor.get_value().unwrap());
            {
                let mut df = conc.lock.lock().unwrap();
                if df.temp != new_temp {
                    df.temp = new_temp;
                    conc.condition.notify_one();
                }
            }
            thread::sleep(POLL_TIME);
        }
    }
}

fn get_wireless_interfaces() -> HashSet<String> {
    let procfile = File::open("/proc/net/wireless").unwrap();
    let reader = BufReader::new(procfile);
    let mut wifs = HashSet::new();
    let wifre = Regex::new(r"^(\w+):").unwrap();
    for line in reader.lines() {
        if let Some(captures) = wifre.captures(&line.unwrap()) {
            wifs.insert(captures.get(1).unwrap().as_str().to_string());
        }
    }
    wifs
}

fn get_current_interface() -> Result<String> {
    lazy_static! {
        static ref RE: Regex = Regex::new(r"dev (\w+) ").unwrap();
    }
    let output = Command::new("ip")
        .arg("route")
        .arg("get")
        .arg("8.8.8.8")
        .output()
        .chain_err(|| "subprocess")?;
    let output_string = String::from_utf8(output.stdout).chain_err(|| "from_utf8")?;
    if let Some(captures) = RE.captures(&output_string) {
        return Ok(captures.get(1).unwrap().as_str().to_string());
    }
    bail!("No current interface.")
}

fn network_thread(conc: Arc<Concurrency>) {
    let wireless = "📡 ⸱ ";
    let wired = "⇅ ⸱ ";

    let wifs = get_wireless_interfaces();

    let monitor = Command::new("ip")
        .arg("monitor")
        .arg("link")
        .arg("address")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdout = monitor.stdout.unwrap();
    let mut buffer = [0; 1024];

    loop {
        let new_symbol = match get_current_interface() {
            Ok(interface) => {
                if wifs.contains(&interface) {
                    wireless
                } else {
                    wired
                }
            }
            Err(_) => "",
        }
        .to_string();
        {
            let mut df = conc.lock.lock().unwrap();
            df.net = new_symbol;
            conc.condition.notify_one();
        }
        stdout.read(&mut buffer).unwrap();
    }
}

fn get_mute() -> Result<bool> {
    let output = Command::new("pamixer")
        .arg("--get-mute")
        .output()
        .chain_err(|| "subprocess")?;
    let mute_string = String::from_utf8(output.stdout).chain_err(|| "decode")?;
    return Ok(mute_string.trim() == String::from("true"));
}

fn get_volume() -> Result<i32> {
    let output = Command::new("pamixer")
        .arg("--get-volume")
        .output()
        .chain_err(|| "subprocess")?;
    let volume_string = String::from_utf8(output.stdout).chain_err(|| "decode")?;
    return Ok(volume_string.trim().parse().chain_err(|| "parse")?);
}

fn volume() -> String {
    if let Ok(muted) = get_mute() {
        if muted {
            return "🔇 ⸱ ".to_string();
        }
    }

    if let Ok(volume) = get_volume() {
        let speaker = match volume {
            0..=33 => "🔈",
            34..=66 => "🔉",
            _ => "🔊",
        };
        return format!("{} {} ⸱ ", speaker, volume);
    }
    return "".to_string();
}

fn volume_thread(conc: Arc<Concurrency>) {
    let re = Regex::new(r"on sink").unwrap();

    loop {
        let mut monitor = Command::new("pactl")
            .arg("subscribe")
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = monitor.stdout.take().unwrap();
        let mut reader = BufReader::new(stdout);

        'events: loop {
            let new_volume = volume();
            {
                let mut df = conc.lock.lock().unwrap();
                if df.volume != new_volume {
                    df.volume = new_volume;
                    conc.condition.notify_one();
                }
            }
            let mut line = String::new();
            while !re.is_match(&line) {
                if reader.read_line(&mut line).unwrap() == 0 {
                    break 'events;
                }
            }
        }

        monitor.wait().unwrap();
    }
}

fn display_thread(conc: Arc<Concurrency>) {
    let (xconn, screen_num) = xcb::Connection::connect(None).unwrap();
    let setup = xconn.get_setup();
    let screen = setup.roots().nth(screen_num as usize).unwrap();
    let root_window = screen.root();

    loop {
        let new_status;
        {
            let mut df = conc.lock.lock().unwrap();
            df = conc.condition.wait(df).unwrap();
            new_status = df.to_string();
        }
        xcb::xproto::change_property(
            &xconn,
            xcb::xproto::PROP_MODE_REPLACE as u8,
            root_window,
            xcb::xproto::ATOM_WM_NAME,
            xcb::xproto::ATOM_STRING,
            8,
            new_status.as_bytes(),
        );
        xconn.flush();
    }
}

fn main() {
    let conc = Arc::new(Concurrency {
        lock: Mutex::new(DisplayFields {
            time: String::new(),
            systemstat: String::new(),
            temp: String::new(),
            net: String::new(),
            volume: String::new(),
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

    {
        let conc2 = Arc::clone(&conc);
        thread::spawn(move || network_thread(conc2));
    }

    {
        let conc2 = Arc::clone(&conc);
        thread::spawn(move || volume_thread(conc2));
    }

    display_thread(conc);
}
