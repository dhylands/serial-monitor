#![recursion_limit = "256"]

use std::io::Write;

use futures::{future::FutureExt, select, StreamExt};

use crossterm::{
    event::{Event, EventStream, KeyCode, KeyEvent, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode},
};

use serialport::{
    DataBits, FlowControl, Parity, SerialPort, SerialPortInfo, SerialPortType, StopBits,
    UsbPortInfo,
};
use structopt::StructOpt;

use tokio_util::codec::{BytesCodec, Decoder};
use wildmatch::WildMatch;

mod error;

use error::{ProgramError, Result};

#[derive(StructOpt, Debug)]
#[structopt(name = "serial-monitor")]
struct Opt {
    /// Filter based on name of port
    #[structopt(short, long)]
    port: Option<String>,

    /// Baud rate to use.
    #[structopt(short, long, default_value = "115200")]
    baud: u32,

    /// Turn on debugging
    #[structopt(short, long)]
    debug: bool,

    /// Turn on local echo
    // #[structopt(short, long)]
    // echo: bool,

    /// List USB Serial devices which are currently connected
    #[structopt(short, long)]
    list: bool,

    /// Turn on verbose messages
    #[structopt(short, long)]
    verbose: bool,

    /// Exit using Control-Y rather than Control-X
    #[structopt(short = "y")]
    ctrl_y_exit: bool,

    /// Filter based on Vendor ID (VID)
    #[structopt(long)]
    vid: Option<String>,

    /// Filter based on Product ID (PID)
    #[structopt(long)]
    pid: Option<String>,

    /// Filter based on Manufacturer name
    #[structopt(short, long)]
    manufacturer: Option<String>,

    /// Filter based on serial number
    #[structopt(short, long)]
    serial: Option<String>,

    /// Filter based on product name
    #[structopt(long)]
    product: Option<String>,
}

fn exit_char(opt: &Opt) -> char {
    if opt.ctrl_y_exit {
        'y'
    } else {
        'x'
    }
}

fn exit_code(opt: &Opt) -> Event {
    Event::Key(KeyEvent {
        code: KeyCode::Char(exit_char(opt)),
        modifiers: KeyModifiers::CONTROL,
    })
}

fn exit_label(opt: &Opt) -> String {
    format!("Control-{}", exit_char(opt).to_ascii_uppercase())
}

fn hex_str(bytes: &[u8]) -> String {
    let mut hex = String::from("");
    let mut ascii = String::from("");

    for byte in bytes.iter() {
        hex.push_str(&format!("{:02x} ", *byte));

        if *byte < 0x20 {
            if *byte == 0x1b {
                if !ascii.is_empty() {
                    ascii.push(' ');
                }
                ascii.push_str("ESC");
            } else {
                if !ascii.is_empty() {
                    ascii.push(' ');
                }
                ascii.push_str("Ctrl-");
                let ctrl = b"@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_";
                ascii.push(ctrl[*byte as usize] as char);
            }
        }
    }

    hex.push(' ');
    hex.push_str(&ascii);
    hex
}

fn matches(str: &str, pattern: Option<String>, opt: &Opt) -> bool {
    let result = match pattern.clone() {
        Some(pattern) => {
            if pattern.contains('*') || pattern.contains('?') {
                // If any wildcards are present, then we assume that the
                // pattern is fully specified
                WildMatch::new(&pattern).is_match(&str)
            } else {
                // Since so wildcard were specified we treat it as if there
                // was a '*' at each end.
                WildMatch::new(&format!("*{}*", pattern)).is_match(&str)
            }
        }
        None => {
            // If no pattern is specified, then we consider that
            // a match has taken place.
            true
        }
    };
    if opt.debug {
        println!(
            "matches(str:{:?}, pattern:{:?}) -> {:?}",
            str, pattern, result
        );
    }
    result
}

fn matches_opt(str: Option<String>, pattern: Option<String>, opt: &Opt) -> bool {
    if let Some(str) = str {
        matches(&str, pattern, opt)
    } else {
        false
    }
}

fn is_usb_serial(port: &SerialPortInfo, opt: &Opt) -> Option<UsbPortInfo> {
    if let SerialPortType::UsbPort(info) = &port.port_type {
        if matches(&port.port_name, opt.port.clone(), opt)
            && matches(&format!("{:04x}", info.vid), opt.vid.clone(), opt)
            && matches(&format!("{:04x}", info.pid), opt.pid.clone(), opt)
            && matches_opt(info.manufacturer.clone(), opt.manufacturer.clone(), opt)
            && matches_opt(info.serial_number.clone(), opt.serial.clone(), opt)
            && matches_opt(info.product.clone(), opt.product.clone(), opt)
        {
            return Some(info.clone());
        }
    }
    None
}

fn extra_usb_info(info: &UsbPortInfo) -> String {
    let mut output = String::new();
    output = output + &format!(" {:04x}:{:04x}", info.vid, info.pid);
    let mut extra_items = Vec::new();

    if let Some(manufacturer) = &info.manufacturer {
        extra_items.push(format!("manufacturer '{}'", manufacturer));
    }
    if let Some(serial) = &info.serial_number {
        extra_items.push(format!("serial '{}'", serial));
    }
    if let Some(product) = &info.product {
        extra_items.push(format!("product '{}'", product));
    }
    if !extra_items.is_empty() {
        output += " with ";
        output += &extra_items.join(" ");
    }
    output
}

fn list_ports(opt: &Opt) {
    if let Ok(ports) = serialport::available_ports() {
        let mut port_found = false;
        for p in ports {
            if let Some(info) = is_usb_serial(&p, &opt) {
                port_found = true;
                println!(
                    "USB Serial Device{} found @{}",
                    extra_usb_info(&info),
                    p.port_name
                );
            }
        }
        if !port_found {
            println!("No USB serial ports found");
        }
    } else {
        println!("Error listing serial ports");
    }
}

fn find_port(opt: &Opt) -> Option<String> {
    if let Ok(ports) = serialport::available_ports() {
        for port in ports {
            if let Some(_info) = is_usb_serial(&port, &opt) {
                return Some(port.port_name);
            }
        }
    }
    None
}

fn handle_key_event(key_event: KeyEvent, tx_port: &mut dyn SerialPort, opt: &Opt) -> Result<()> {
    if opt.debug {
        println!("Event::{:?}\r", key_event);
    }

    // The following escape sequeces cone from the MicroPython codebase.
    //
    //  Up      ESC [A
    //  Down    ESC [B
    //  Right   ESC [C
    //  Left    ESC [D
    //  Home    ESC [H  or ESC [ 1 ~
    //  End     ESC [F  or ESC [ 4 ~
    //  Del     ESC [ 3 ~
    //  Insert  ESC [ 2 ~

    let mut buf = [0; 4];

    let key_str: Option<&[u8]> = match key_event.code {
        KeyCode::Backspace => Some(b"\x08"),
        KeyCode::Enter => Some(b"\x0D"),
        KeyCode::Left => Some(b"\x1b[D"),
        KeyCode::Right => Some(b"\x1b[C"),
        KeyCode::Up => Some(b"\x1b[A"),
        KeyCode::Down => Some(b"\x1b[B"),
        KeyCode::Tab => Some(b"\x09"),
        KeyCode::Delete => Some(b"\x1b[3~"),
        KeyCode::Insert => Some(b"\x1b[2~"),
        KeyCode::Esc => Some(b"\x1b"),
        KeyCode::Char(ch) => {
            if key_event.modifiers & KeyModifiers::CONTROL == KeyModifiers::CONTROL {
                buf[0] = ch as u8;
                if (ch >= 'a' && ch <= 'z') || (ch == ' ') {
                    buf[0] &= 0x1f;
                    Some(&buf[0..1])
                } else if ch >= '4' && ch <= '7' {
                    // crossterm returns Control-4 thru 7 for \x1c thru \x1f
                    buf[0] = (buf[0] + 8) & 0x1f;
                    Some(&buf[0..1])
                } else {
                    None
                }
            } else {
                Some(ch.encode_utf8(&mut buf).as_bytes())
            }
        }
        _ => None,
    };
    if let Some(key_str) = key_str {
        if opt.debug {
            println!("{}\r", hex_str(key_str));
        }
        tx_port.write_all(key_str)?;
    }

    Ok(())
}

async fn monitor(port: &mut tokio_serial::Serial, opt: &Opt) -> Result<()> {
    let mut reader = EventStream::new();
    let mut tx_port = port.try_clone()?;
    let mut serial_reader = BytesCodec::new().framed(port);

    let exit_code = exit_code(opt);

    loop {
        let mut event = reader.next().fuse();
        let mut serial_event = serial_reader.next().fuse();

        select! {
            maybe_event = event => {
                match maybe_event {
                    Some(Ok(event)) => {
                        if event == exit_code {
                            break;
                        }
                        if let Event::Key(key_event) = event {
                            handle_key_event(key_event, tx_port.as_mut(), opt)?;
                        } else {
                            println!("Unrecognized Event::{:?}\r", event);
                        }
                    }
                    Some(Err(e)) => println!("Error: {:?}\r", e),
                    None => break,
                }
            },
            maybe_serial = serial_event => {
                match maybe_serial {
                    Some(Ok(serial_event)) => {
                        if opt.debug {
                            println!("Serial Event:{:?}\r", serial_event);
                        } else {
                            print!("{}", String::from_utf8_lossy(&serial_event));
                            std::io::stdout().flush()?;
                        }
                    },
                    Some(Err(e)) => println!("Serial Error: {:?}\r", e),
                    None => break,
                }
            },
        };
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let opt = Opt::from_args();

    if opt.verbose {
        println!("{:#?}", opt);
    }

    if opt.list {
        list_ports(&opt);
        return Ok(());
    }

    let mut settings = tokio_serial::SerialPortSettings::default();
    settings.baud_rate = opt.baud;
    settings.data_bits = DataBits::Eight;
    settings.parity = Parity::None;
    settings.stop_bits = StopBits::One;
    settings.flow_control = FlowControl::None;

    if let Some(port_name) = find_port(&opt) {
        let err_port_name = port_name.clone();
        let mut port = tokio_serial::Serial::from_path(port_name.clone(), &settings)
            .map_err(|e| ProgramError::UnableToOpen(err_port_name, e))?;

        println!("Connected to {}", port_name);
        println!("Press {} to exit", exit_label(&opt));
        enable_raw_mode()?;
        let result = monitor(&mut port, &opt).await;
        disable_raw_mode()?;
        println!();
        return result;
    }

    Err(ProgramError::MustSpecifyPort)
}
