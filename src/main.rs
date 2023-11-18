#![recursion_limit = "256"] // Needed for select!

use std::{
    convert::TryFrom,
    fmt::Display,
    io::{self, Write},
    ops::Deref,
    process::ExitCode,
    result::Result as StdResult,
};

use bytes::Bytes;
use clap::{Parser, ValueEnum};
use crossterm::{
    event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode},
};
use eyre::WrapErr;
use futures::{future::FutureExt, select, StreamExt};
use mio_serial::SerialPortInfo;
use serialport::{SerialPortType, UsbPortInfo};
use tokio_serial::{DataBits, FlowControl, Parity, SerialPortBuilderExt, StopBits};
use tokio_util::codec::BytesCodec;
use wildmatch::WildMatch;

mod string_decoder;
use string_decoder::StringDecoder;

#[derive(Parser, Debug)]
#[command(name = "serial-monitor", version, about)]
struct Opt {
    /// Filter based on name of port
    #[arg(short, long)]
    port: Option<String>,

    /// Baud rate to use
    #[arg(short, long, default_value = "115200")]
    baud: u32,

    /// Turn on debugging
    #[arg(short, long)]
    debug: bool,

    // Turn on local echo
    #[arg(short, long)]
    echo: bool,

    /// List USB serial devices which are currently connected
    #[arg(short, long)]
    list: bool,

    /// Enter character to send (cr, lf, crlf)
    #[arg(long, default_value = "cr")]
    enter: Eol,

    /// Like list, but only prints the name of the port that was found.
    /// This is useful for using from scripts or makefiles.
    #[arg(short, long)]
    find: bool,

    /// Turn on verbose messages
    #[arg(short, long)]
    verbose: bool,

    /// Exit using Control-Y rather than Control-X
    #[arg(short = 'y')]
    ctrl_y_exit: bool,

    /// Filter based on Vendor ID (VID)
    #[arg(long)]
    vid: Option<String>,

    /// Filter based on Product ID (PID)
    #[arg(long)]
    pid: Option<String>,

    /// Filter based on manufacturer name
    #[arg(short, long)]
    manufacturer: Option<String>,

    /// Filter based on serial number
    #[arg(short, long)]
    serial: Option<String>,

    /// Filter based on product name
    #[arg(long)]
    product: Option<String>,

    /// Return the index'th result
    #[arg(long)]
    index: Option<usize>,

    /// Parity checking (none, odd, even)
    #[arg(long, default_value = "none")]
    parity: ParityOpt,

    /// Stop bits (1, 2)
    #[arg(long, default_value = "1")]
    stopbits: usize,

    /// Flow control (none, software, hardware)
    #[arg(long, default_value = "none")]
    flow: FlowControlOpt,

    /// Data bits (5, 6, 7, 8)
    #[arg(long, default_value = "8")]
    databits: usize,
}

struct DataBitsOpt(DataBits);

impl TryFrom<usize> for DataBitsOpt {
    type Error = io::Error;

    fn try_from(value: usize) -> StdResult<Self, io::Error> {
        match value {
            5 => Ok(Self(DataBits::Five)),
            6 => Ok(Self(DataBits::Six)),
            7 => Ok(Self(DataBits::Seven)),
            8 => Ok(Self(DataBits::Eight)),
            _ => Err(io::Error::new(
                io::ErrorKind::Other,
                "databits out of range",
            )),
        }
    }
}

/// Flow control modes
#[derive(Clone, Copy, Debug, ValueEnum, strum::EnumString, strum::EnumVariantNames)]
#[strum(serialize_all = "snake_case")]
enum FlowControlOpt {
    /// No flow control.
    None,
    /// Flow control using XON/XOFF bytes.
    Software,
    /// Flow control using RTS/CTS signals.
    Hardware,
}

impl From<FlowControlOpt> for FlowControl {
    fn from(opt: FlowControlOpt) -> Self {
        match opt {
            FlowControlOpt::None => FlowControl::None,
            FlowControlOpt::Software => FlowControl::Software,
            FlowControlOpt::Hardware => FlowControl::Hardware,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum, strum::EnumString, strum::EnumVariantNames)]
#[strum(serialize_all = "snake_case")]
enum ParityOpt {
    /// No parity bit.
    None,
    /// Parity bit sets odd number of 1 bits.
    Odd,
    /// Parity bit sets even number of 1 bits.
    Even,
}

impl From<ParityOpt> for Parity {
    fn from(opt: ParityOpt) -> Self {
        match opt {
            ParityOpt::None => Parity::None,
            ParityOpt::Odd => Parity::Odd,
            ParityOpt::Even => Parity::Even,
        }
    }
}

struct StopBitsOpt(StopBits);

impl TryFrom<usize> for StopBitsOpt {
    type Error = io::Error;

    fn try_from(value: usize) -> StdResult<Self, io::Error> {
        match value {
            1 => Ok(Self(StopBits::One)),
            2 => Ok(Self(StopBits::Two)),
            _ => Err(io::Error::new(
                io::ErrorKind::Other,
                "stopbits out of range",
            )),
        }
    }
}

/// End of line character options
#[derive(Debug, Clone, Copy, ValueEnum, strum::EnumString, strum::EnumVariantNames)]
#[strum(serialize_all = "snake_case")]
enum Eol {
    /// Carriage return
    Cr,
    /// Carriage return, line feed
    Crlf,
    /// Line feed
    Lf,
}

impl Eol {
    fn bytes(&self) -> &[u8] {
        match self {
            Self::Cr => &b"\r"[..],
            Self::Crlf => &b"\r\n"[..],
            Self::Lf => &b"\n"[..],
        }
    }
}

// Returns the lowercase version of the character which will cause
// serial-monitor to exit.
fn exit_char(opt: &Opt) -> char {
    if opt.ctrl_y_exit {
        'y'
    } else {
        'x'
    }
}

// Returns the Event::Key variant of the exit character which will
// cause the serial monitor to exit.
fn exit_code(opt: &Opt) -> Event {
    Event::Key(KeyEvent {
        code: KeyCode::Char(exit_char(opt)),
        modifiers: KeyModifiers::CONTROL,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    })
}

// Returns a human readable string of the exit character.
fn exit_label(opt: &Opt) -> String {
    format!("Control-{}", exit_char(opt).to_ascii_uppercase())
}

// Converts a byte string into a string comprised of each byte
// in hexadecimal, followed by a more human readable ASCII variant.
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
        } else if *byte > b'~' {
            ascii.push('.');
        } else {
            ascii.push(*byte as char);
        }
    }

    hex.push(':');
    hex.push(' ');
    hex.push_str(&ascii);
    hex
}

// Checks to see if a string matches a pattern used for filtering.
fn matches(str: &str, pattern: Option<String>, opt: &Opt) -> bool {
    let result = match pattern.clone() {
        Some(pattern) => {
            if pattern.contains('*') || pattern.contains('?') {
                // If any wildcards are present, then we assume that the
                // pattern is fully specified
                WildMatch::new(&pattern).matches(str)
            } else {
                // Since no wildcard were specified we treat it as if there
                // was a '*' at each end.
                WildMatch::new(&format!("*{}*", pattern)).matches(str)
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

// Similar to matches but checks to see if an Option<String> matches a pattern.
fn matches_opt(str: Option<String>, pattern: Option<String>, opt: &Opt) -> bool {
    if let Some(str) = str {
        matches(&str, pattern, opt)
    } else {
        // If no pattern was specified, then we don't care if there was a string
        // supplied or not. But if we're looking for a particular patterm, then
        // it needs to match.
        let result = pattern.is_none();
        if opt.debug {
            println!(
                "matches_opt(str:{:?}, pattern:{:?}) -> {:?}",
                str, pattern, result
            );
        }
        result
    }
}

#[cfg(target_os = "macos")]
fn map_port_name(port_name: &str) -> String {
    // available_ports returns /dev/tty.* rather than /dev/cu.*
    // /dev/tty.* are designed for incoming serial connections and will block
    // until DCD is set.
    // /dev/cu.* are designed for outgoing serial connections and don't block,
    // so we change /dev/tty.* to /dev/cu.* since this program is primarily
    // used for outgoing connections.
    if port_name.starts_with("/dev/tty.") {
        port_name.replace("/dev/tty.", "/dev/cu.")
    } else {
        String::from(port_name)
    }
}

// Returns a list of the available ports (for macos)
#[cfg(target_os = "macos")]
fn available_ports() -> color_eyre::Result<Vec<SerialPortInfo>> {
    Ok(mio_serial::available_ports()?
        .into_iter()
        .map(|mut port| {
            port.port_name = map_port_name(&port.port_name);
            port
        })
        .collect())
}

// Returns a list of the available ports (for everything but macos)
#[cfg(not(target_os = "macos"))]
fn available_ports() -> color_eyre::Result<Vec<SerialPortInfo>> {
    mio_serial::available_ports().wrap_err("couldn't list ports")
}

// Checks to see if a serial port matches the filtering criteria specified on the command line.
fn usb_port_matches(idx: usize, port: &SerialPortInfo, opt: &Opt) -> bool {
    let SerialPortType::UsbPort(info) = &port.port_type else {
        return false;
    };

    if let Some(opt_idx) = opt.index {
        if opt_idx != idx {
            return false;
        }
    }

    matches(&port.port_name, opt.port.clone(), opt)
        && matches(&format!("{:04x}", info.vid), opt.vid.clone(), opt)
        && matches(&format!("{:04x}", info.pid), opt.pid.clone(), opt)
        && matches_opt(info.manufacturer.clone(), opt.manufacturer.clone(), opt)
        && matches_opt(info.serial_number.clone(), opt.serial.clone(), opt)
        && matches_opt(info.product.clone(), opt.product.clone(), opt)
}

fn filtered_ports(opt: &Opt) -> color_eyre::Result<Vec<SerialPortInfo>> {
    let mut ports: Vec<SerialPortInfo> = available_ports()?
        .into_iter()
        .enumerate()
        .filter_map(|(idx, info)| usb_port_matches(idx, &info, opt).then_some(info))
        .collect();

    ports.sort_by(|a, b| a.port_name.cmp(&b.port_name));

    Ok(ports)
}

fn filtered_port(opt: &Opt) -> color_eyre::Result<Option<SerialPortInfo>> {
    let mut ports = filtered_ports(opt)?;

    if ports.is_empty() {
        return Ok(None);
    }

    Ok(Some(ports.swap_remove(0)))
}

// Formats the USB Port information into a human readable form.
struct ExtraInfo<'a>(&'a UsbPortInfo);

impl<'a> Deref for ExtraInfo<'a> {
    type Target = UsbPortInfo;

    fn deref(&self) -> &Self::Target {
        self.0
    }
}

impl<'a> ExtraInfo<'a> {
    fn has_extra(&self) -> bool {
        self.manufacturer.is_some() || self.serial_number.is_some() || self.product.is_some()
    }
}

impl<'a> Display for ExtraInfo<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, " {:04x}:{:04x}", self.vid, self.pid)?;

        if self.has_extra() {
            write!(f, " with")?;

            if let Some(manufacturer) = &self.manufacturer {
                write!(f, " manufacturer '{}'", manufacturer)?;
            }
            if let Some(serial) = &self.serial_number {
                write!(f, " serial '{}'", serial)?;
            }
            if let Some(product) = &self.product {
                write!(f, " product '{}'", product)?;
            }
        }

        Ok(())
    }
}

// Lists all of the USB serial ports which match the filtering criteria.
fn list_ports(opt: &Opt) -> color_eyre::Result<()> {
    for port in filtered_ports(opt)? {
        if let SerialPortType::UsbPort(info) = &port.port_type {
            println!(
                "USB Serial Device{} found @{}",
                ExtraInfo(info),
                port.port_name
            );
        } else {
            println!("Serial Device found @{}", port.port_name);
        }
    }
    Ok(())
}

// Returns the first port which matches the filtering criteria.
fn find_port(opt: &Opt) -> color_eyre::Result<Option<String>> {
    filtered_port(opt).map(|op| op.map(|port| port.port_name))
}

// Converts key events from crossterm into appropriate character/escape sequences which are then
// sent over the serial connection.
fn handle_key_event(key_event: KeyEvent, opt: &Opt) -> color_eyre::Result<Option<Bytes>> {
    if opt.debug {
        println!("Event::{:?}\r", key_event);
    }

    // The following escape sequeces come from the MicroPython codebase.
    //
    //  Up      ESC [A
    //  Down    ESC [B
    //  Right   ESC [C
    //  Left    ESC [D
    //  Home    ESC [H  or ESC [1~
    //  End     ESC [F  or ESC [4~
    //  Del     ESC [3~
    //  Insert  ESC [2~

    let mut buf = [0; 4];

    let key_str: Option<&[u8]> = match key_event.code {
        KeyCode::Backspace => Some(b"\x08"),
        KeyCode::Enter => Some(opt.enter.bytes()),
        KeyCode::Left => Some(b"\x1b[D"),
        KeyCode::Right => Some(b"\x1b[C"),
        KeyCode::Home => Some(b"\x1b[H"),
        KeyCode::End => Some(b"\x1b[F"),
        KeyCode::Up => Some(b"\x1b[A"),
        KeyCode::Down => Some(b"\x1b[B"),
        KeyCode::Tab => Some(b"\x09"),
        KeyCode::Delete => Some(b"\x1b[3~"),
        KeyCode::Insert => Some(b"\x1b[2~"),
        KeyCode::Esc => Some(b"\x1b"),
        KeyCode::Char(ch) => {
            if key_event.modifiers & KeyModifiers::CONTROL == KeyModifiers::CONTROL {
                buf[0] = ch as u8;
                if ch.is_ascii_lowercase() || (ch == ' ') {
                    buf[0] &= 0x1f;
                    Some(&buf[0..1])
                } else if ('4'..='7').contains(&ch) {
                    // crossterm returns Control-4 thru 7 for \x1c thru \x1f
                    buf[0] = (buf[0] + 8) & 0x1f;
                    Some(&buf[0..1])
                } else {
                    Some(ch.encode_utf8(&mut buf).as_bytes())
                }
            } else {
                Some(ch.encode_utf8(&mut buf).as_bytes())
            }
        }
        _ => None,
    };
    if let Some(key_str) = key_str {
        if opt.debug {
            println!("Send: {}\r", hex_str(key_str));
        }
        if opt.echo {
            if let Ok(val) = std::str::from_utf8(key_str) {
                print!("{}", val);
                std::io::stdout().flush()?;
            }
        }
        Ok(Some(Bytes::copy_from_slice(key_str)))
    } else {
        Ok(None)
    }
}

// Main function which collects input from the user and sends it over the serial link
// and collects serial data and presents it to the user.
async fn monitor(port: &mut tokio_serial::SerialStream, opt: &Opt) -> color_eyre::Result<()> {
    let mut reader = EventStream::new();
    let (rx_port, tx_port) = tokio::io::split(port);

    let mut serial_reader = tokio_util::codec::FramedRead::new(rx_port, StringDecoder::new());
    let serial_sink = tokio_util::codec::FramedWrite::new(tx_port, BytesCodec::new());
    let (serial_writer, serial_consumer) = futures::channel::mpsc::unbounded::<Bytes>();

    let exit_code = exit_code(opt);

    let mut poll_send = serial_consumer.map(Ok).forward(serial_sink);
    loop {
        let mut event = reader.next().fuse();
        let mut serial_event = serial_reader.next().fuse();

        select! {
            _ = poll_send => {}
            maybe_event = event => {
                match maybe_event {
                    Some(Ok(event)) => {
                        if event == exit_code {
                            break;
                        }
                        if let Event::Key(key_event) = event {
                            if let Some(key) = handle_key_event(key_event, opt)? {
                                serial_writer.unbounded_send(key).unwrap();
                            }
                        } else {
                            println!("Unrecognized Event::{:?}\r", event);
                        }
                    }
                    Some(Err(e)) => println!("crossterm Error: {:?}\r", e),
                    None => {
                        println!("maybe_event returned None\r");
                    },
                }
            },
            maybe_serial = serial_event => {
                match maybe_serial {
                    Some(Ok(serial_event)) => {
                        if opt.debug {
                            println!("Serial Event:{:?}\r", serial_event);
                        } else {
                            print!("{}", serial_event);
                            std::io::stdout().flush()?;
                        }
                    },
                    Some(Err(e)) => {
                        println!("Serial Error: {:?}\r", e);
                        // This most likely means that the serial port has been unplugged.
                        break;
                    },
                    None => {
                        println!("maybe_serial returned None\r");
                    },
                }
            },
        };
    }

    Ok(())
}

// Main entry point to the program.
#[tokio::main]
async fn main() -> color_eyre::Result<ExitCode> {
    let opt = Opt::parse();

    color_eyre::install()?;

    if opt.verbose {
        eprintln!("{:#?}", opt);
    }

    if opt.list {
        list_ports(&opt)?;
        return Ok(ExitCode::SUCCESS);
    }

    let Some(port_name) = find_port(&opt)? else {
        eprintln!("No USB ports found");
        return Ok(ExitCode::from(1));
    };

    if opt.find {
        println!("{port_name}");
        return Ok(ExitCode::SUCCESS);
    }

    // Do the serial port monitoring
    let mut port = tokio_serial::new(&port_name, opt.baud)
        .data_bits(DataBitsOpt::try_from(opt.databits)?.0)
        .parity(Parity::from(opt.parity))
        .stop_bits(StopBitsOpt::try_from(opt.stopbits)?.0)
        .flow_control(FlowControl::from(opt.flow))
        .open_native_async()
        .wrap_err_with(|| format!("Unable to open {port_name}"))?;

    println!("Connected to {}", port_name);
    println!("Press {} to exit", exit_label(&opt));
    enable_raw_mode()?;
    let result = monitor(&mut port, &opt).await;
    disable_raw_mode()?;
    println!();

    result?;

    Ok(ExitCode::SUCCESS)
}
