/*
 * Time: at the top of every minute
 * RAM/CPU/Temp: every 5s
 * Network: ip monitor
 * Volume: pactl subscribe
 */

extern crate chrono;

use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time;

#[derive(Debug)]
struct DisplayFields {
    time: String,
}

#[derive(Debug)]
struct Concurrency {
    lock: Mutex<DisplayFields>,
    condition: Condvar,
}

fn time_thread(conc: Arc<Concurrency>) {
    loop {
        {
            let mut df = conc.lock.lock().unwrap();
            df.time = chrono::Local::now()
                .format("📆 %a, %d %h ⸱ 🕓 %R")
                .to_string();
            conc.condition.notify_one();
        }
        let now = chrono::Local::now();
        let now_ts = now.timestamp_millis();
        let next_ts = ((now_ts / 60_000) + 1) * 60_000;
        let sleep_time = time::Duration::from_millis((next_ts - now_ts) as u64);
        thread::sleep(sleep_time);
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
            time: "".to_string(),
        }),
        condition: Condvar::new(),
    });

    {
        let conc2 = Arc::clone(&conc);
        thread::spawn(move || time_thread(conc2));
    }

    display_thread(conc);
}
