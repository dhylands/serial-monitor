use serialport::{SerialPortInfo, SerialPortType, UsbPortInfo};
use std::path::PathBuf;
use structopt::StructOpt;

#[derive(StructOpt, Debug)]
#[structopt(name = "usb-ser-mon")]
struct Opt {
    /// Serial port to open
    #[structopt(short, long)]
    port: Option<String>,

    /// Baud rate to use.
    #[structopt(short, long, default_value = "115200")]
    baud: u32,

    /// Turn on debugging
    #[structopt(short, long)]
    debug: bool,

    /// Turn on local echo
    #[structopt(short, long)]
    echo: bool,

    // List USB Serial devices which are currently connected
    #[structopt(short, long)]
    list: bool,

    /// Turn on verbose messages
    #[structopt(short, long)]
    verbose: bool,
}

fn is_usb_serial(port: &SerialPortInfo, opt: &Opt) -> Option<UsbPortInfo> {
    if let SerialPortType::UsbPort(info) = &port.port_type {
        if let Some(port_name) = &opt.port {
            if !port.port_name.contains(port_name) {
                return None;
            }
        }
        return Some(info.clone());
    }
    None
}

fn extra_info(port: &SerialPortInfo) -> String {
    let mut output = String::new();
    if let SerialPortType::UsbPort(info) = &port.port_type {
        output = output + &format!(" {:04x}:{:04x}", info.vid, info.pid);
        let mut extra_items = Vec::new();

        if let Some(manufacturer) = &info.manufacturer {
            extra_items.push(format!("vendor '{}'", manufacturer));
        }
        if let Some(serial) = &info.serial_number {
            extra_items.push(format!("serial '{}'", serial));
        }
        if let Some(product) = &info.product {
            extra_items.push(format!("product '{}'", product));
        }
        if extra_items.len() > 0 {
            output += " with ";
            output += &extra_items.join(" ");
        }
    }
    output
}

fn list_ports(opt: &Opt) {
    if let Ok(ports) = serialport::available_ports() {
        let mut port_found = false;
        for p in ports {
            if let Some(info) = is_usb_serial(&p, &opt) {
                port_found = true;
                println!("USB Serial Device{} found @{}", extra_info(&p), p.port_name);
            }
        }
        if !port_found {
            println!("No USB serial ports found");
        }
    } else {
        print!("Error listing serial ports");
    }
}

fn main() {
    let opt = Opt::from_args();

    if opt.verbose {
        println!("{:#?}", opt);
    }

    if opt.list {
        list_ports(&opt);
    }
}
