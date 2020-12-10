use std::{thread, time};
use std::fs::read_dir;
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::sync::mpsc::{Receiver, Sender};
use std::thread::{JoinHandle};
use std::ops::{Deref, DerefMut};
use crate::State::{Watching, InSync};

#[derive(Debug)]
enum State {
    InSync,
    Watching,
}

struct Event {
    display: String,
    primary: String,
    clipboard: String,
}

fn main() {
    println!("Welcome in clipboard sync : ");
    println!("Found displays: {:?}", get_displays());
    let in_sync: Arc<Mutex<Option<State>>> = Arc::new(Mutex::new(Some(Watching)));
    let (handles, rx) = register_clipboard_watchers(&in_sync);
    sync_clipboards(rx, &in_sync);
    for handle in handles {
        handle.join().unwrap();
    }
}

fn sync_clipboards(rx: Vec<Receiver<Event>>, in_sync: &Arc<Mutex<Option<State>>>) {
    for event in rx {
        let lock = Arc::clone(&in_sync);
        thread::spawn(move || {
            sync(event, lock)
        });
    }
}

fn register_clipboard_watchers(in_sync: &Arc<Mutex<Option<State>>>) -> (Vec<JoinHandle<()>>, Vec<Receiver<Event>>) {
    let mut output_channels = Vec::new();
    let mut handles: Vec<JoinHandle<()>> = Vec::new();
    for display in get_displays() {
        let (tx_clip, rx_clip) = mpsc::channel();
        output_channels.push(rx_clip);
        let in_sync = Arc::clone(&in_sync);
        handles.push(thread::spawn(move || {
            watch_clip_notify(&display, tx_clip, in_sync);
        }));
    }
    (handles, output_channels)
}

fn sync(channel: Receiver<Event>, lock: Arc<Mutex<Option<State>>>) {
    for received in channel {
        let mut guard = lock.lock().unwrap();
        let in_sync = guard.deref_mut();
        in_sync.replace(InSync);
        drop(guard);
        for display in get_displays() {
            if display != received.display {
                sync_values(display, &received);
            }
        }
        thread::sleep(time::Duration::from_millis(300));
        let mut guard = lock.lock().unwrap();
        let in_sync = guard.deref_mut();
        in_sync.replace(Watching);
        drop(guard);
    }
}

/// Get all registered X11 unix displays. (reading /tmp/.X11-unix directory)
/// # Returns
/// A `Vec<String>` that holds each display named e.g: [":0", ":1"]
fn get_displays() -> Vec<String> {
    read_dir("/tmp/.X11-unix")
        .unwrap()
        .map(|element| { element.unwrap().file_name().into_string() })
        .map(|element| { element.unwrap_or_default().replace("X", ":") })
        .collect()
}

/// Watch events emitted by clipboard for specified `display` using `clipnotify` package
/// and forward them to our rust application
///
/// # Arguments
/// * `display` - A `&String` that holds the name of the virtual display to watch
/// * `tx`      - A `Sender<Event>` that is designed to forward notification
/// * `in_sync` - A `Arc<Mutex<Option<State>>>` that holds the in sync state of our app
fn watch_clip_notify(display: &String, tx: Sender<Event>, in_sync: Arc<Mutex<Option<State>>>) {
    println!("start {} watcher", display);
    loop {
        Command::new("clipnotify")
            .env("DISPLAY", display)
            .output()
            .expect("failed to execute process");
        let event = Event {
            display: display.clone(),
            primary: read_primary_value(display),
            clipboard: read_clipboard_value(display),
        };
        let guard = in_sync.lock().unwrap();
        let syncing = guard.deref().as_ref().unwrap();
        println!("app is {:?}", syncing);
        match syncing {
            Watching => {
                drop(guard);
                tx.send(event).unwrap()
            }
            _ => {
                drop(guard);
            }
        }
    }
}


/// Reads value contained in clipboard for specified `display`
/// As linux clipboard may contain different `selection` this arg allows
/// to specify the one we wanted to read (`primary` selection, or `clipboard` in our case)
///
/// # Arguments
/// * `display`        - A `&String` that holds the name of the virtual display were the value is read
/// * `selection_type` - A `$str` to specify which of selection type to use (see xclip documentation for more details)
fn read_xclip_value(display: &String, selection_type: &str) -> String {
    String::from_utf8(Command::new("xclip")
        .arg("-selection").arg(selection_type).arg("-out").arg("-display").arg(display)
        .output()
        .expect(&*format!("failed to retrieve clipboard content for {} display", display))
        .stdout
    ).unwrap()
}

/// Allows to write a value in clipboard for specified `display`
/// as for `read_xclip_value` method, this one takes `selection` argument
/// and a `value` argument.
///
/// * `display`        - A `&String` that holds the name of the virtual display were the value is read
/// * `selection_type` - A `&str` to specify which of selection type to use (see xclip documentation for more details)
/// * `value`          - A `&String` that holds the value to write in the specified selection
fn write_xclip_value(display: &String, selection_type: &str, value: &String) {
    let mut process = Command::new("xclip")
        .arg("-selection").arg(selection_type).arg("-in").arg("-display").arg(display)
        .stdin(Stdio::piped()).spawn().unwrap();
    let stdin = process.stdin.as_mut().unwrap();
    stdin.write_all(value.as_bytes()).unwrap();
    let result = process.wait();
    if result.is_err() {
        println!("Error during copy of {}:{} value to {} display", selection_type, value, display);
    }
}

fn write_primary_value(display: &String, value: &String) {
    write_xclip_value(display, "primary", value);
}

fn write_clipboard_value(display: &String, value: &String) {
    write_xclip_value(display, "clipboard", value);
}

fn read_primary_value(display: &String) -> String {
    read_xclip_value(display, "primary")
}

fn read_clipboard_value(display: &String) -> String {
    read_xclip_value(display, "clipboard")
}

fn sync_values(display: String, event: &Event) {
    write_clipboard_value(&display, &event.clipboard);
    write_primary_value(&display, &event.primary);
    println!("sync DISPLAY={} with DISPLAY={} values:  primary={} clipboard={}", display, event.display, event.primary, event.clipboard)
}
