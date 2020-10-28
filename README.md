serial-monitor
==============

A serial monitor for USB Serial devices, written in rust.

`serial-monitor` is a command line program which will conect to, and allow you to interact with devices
which are connected to your host computer via USB serial adapters.

`serial-monitor` has been tested on Mac OSX, and Linux. Currently there is an
[issue](https://github.com/berkowski/mio-serial/issues/22)
in mio-serial which prevents this from running properly on Windows.

You can use the `--list` option to display all of the detected USB serial
adapters, and you can use the `--vid`, `--pid`, `--port`, `--serial`, 
`--manufacturer`, or `--product` options to filter your list.

`serial-monitor` will open the first USB serial port which satifies the
filtering criteria.

Once, connected, you can use Control-X (or Control-Y if started with the -y option)
to exit from `serial-monitor` and return you to your prompt.

Installation
============

Install rust. The recommended way is to use [rustup](https://rustup.rs/).

Once rust is installed you can install directly from crates.io by using:
```
cargo install serial-monitor
```
This installs serial-monitor into `~/.cargo/bin`

If you want to build and install from source locally, then clone the
[repository](https://github.com/dhylands/serial-monitor.git):
```
git clone https://github.com/dhylands/serial-monitor.git
cd serial-monitor
```

You can then build and install `serial-monitor` using:
```
cargo install --path .
```
This will build `serial-monitor` and install it into `~/.cargo/bin`

Usage
=====

```
$ serial-monitor --help
serial-monitor 0.0.5

USAGE:
    serial-monitor [FLAGS] [OPTIONS]

FLAGS:
    -y               Exit using Control-Y rather than Control-X
    -d, --debug      Turn on debugging
    -e, --echo
    -f, --find       Like list, but only prints the name of the port that was found. This is useful for using from
                     scripts or makefiles
    -h, --help       Prints help information
    -l, --list       List USB Serial devices which are currently connected
    -V, --version    Prints version information
    -v, --verbose    Turn on verbose messages

OPTIONS:
    -b, --baud <baud>                    Baud rate to use [default: 115200]
        --databits <databits>            Data bits (5, 6, 7, 8) [default: 8]
        --flow <flow-control>            Flow control (none, software, hardware) [default: none]
        --index <index>                  Return the index'th result
    -m, --manufacturer <manufacturer>    Filter based on Manufacturer name
        --parity <parity>                Parity checking (none, odd, even) [default: none]
        --pid <pid>                      Filter based on Product ID (PID)
    -p, --port <port>                    Filter based on name of port
        --product <product>              Filter based on product name
    -s, --serial <serial>                Filter based on serial number
        --stopbits <stopbits>            Stop bits (1, 2) [default: 1]
        --vid <vid>                      Filter based on Vendor ID (VID)
```

The `--list` (or `-l`) option will list all of the connected USB Serial Adapters, for example:
```
$ serial-monitor --list
USB Serial Device f055:9800 with manufacturer 'MicroPython' serial '336F338F3433' product 'Pyboard Virtual Comm Port in FS Mode' found @/dev/cu.usbmodem336F338F34332
USB Serial Device 0403:6001 with manufacturer 'FTDI' serial 'A700e6Lr' product 'FT232R USB UART' found @/dev/cu.usbserial-A700e6Lr
```

Monitoring the serial port
==========================

To connect to the MicroPython board (assuming the list shown above), you might do: `serial-monitor -m Micro` or perhaps `serial-monitor --vid f055`.
You can experiment with filtering options by combining with the `--list` option which will only show you the USB Serial Adapters which
match the filter criteria.

Once you're connected you should see something like the following:
```
Connected to /dev/cu.usbmodem336F338F34332
Press Control-X to exit
MicroPython v1.11-47-g1a51fc9dd on 2019-06-18; PYBv1.1 with STM32F405RG
Type "help()" for more information.
>>> 
```

To exit from `serial-monitor` use Control-X (or Control-Y if you started with the `-y` option). Using Control-X allows characters like Control-C and Control-D
to be passed on to the device on the serial port.

Filtering ports
===============

You can use the `--vid`, `--pid`, `--port`, `--serial`, `--manufacturer`, or `--product` options to filter the selection of the serial ports.
All of these options allow the use of `*` or `?` wildcards. Note that you'll probably need to quote these to prevent your shell from expanding them.
`*` means to match 0 or more characters and `?` means to match one character. If you don't specify any wildcards then it is assumed that there is a `*`
at the beginning and the end of the string. So `--product FS` will behave as if you had typed `--product '*FS*'`.

The `--index` option can be used to return the index'th result. This is useful for devices like the Black Magic Probe which return multiple
serial ports which all have identical attributes.

Finding ports from within a script
===============================

The `--find` option behaves very similary to the `--list` command, but it only displays the port name of the first port found.

For example, `serial-monitor --list` might show:
```
USB Serial Device 1d50:6018 with manufacturer 'Black Sphere Technologies' serial '7ABA4DC1' product 'Black Magic Probe' found @/dev/cu.usbmodem7ABA4DC11
USB Serial Device 1d50:6018 with manufacturer 'Black Sphere Technologies' serial '7ABA4DC1' product 'Black Magic Probe' found @/dev/cu.usbmodem7ABA4DC13
```
and the command `serial-monitor --find --product 'Black Magic Probe' --index 0` would show:
```
/dev/cu.usbmodem7ABA4DC11
```

This can be quite useful from within a script:
```bash
GDB_PORT = $(serial-monitor --find --product 'Black Magic Probe' --index 0)
arm-none-eabi-gdb -ex 'target extended-remote ${GDB_PORT}' -x gdbinit myprogram.elf
```

Supporting Home/End keys on Mac OS
==================================

On the regular Mac keyboard Fn-Left generates `ESC [ H` (which corresponds to Home) and
Fn-Right generates `ESC [ F` (which corresponds to End). For some reason, pressing the
Home and End keys on an external keyboard are mapped to `ESC O H` and `ESC O F` and
crossterm wasn't reporting any events when these keys were pressed. If you go into
Terminal->Preferences, select your Profile and then the Keyboard tab, you can change
Home from `ESC O H` to be `ESC [ H` and End from `ESC O F` to be `ESC [ F` then the
Home and End keys are reported properly via crossterm. Here's a picture of what the
Home and End keys look like in the Keyboard tab:

![Home/End](https://github.com/dhylands/serial-monitor/raw/master/MacOS-Home-End.png)

This has the nice side-effect of making the Home/End keys work properly in bash in the Mac's Terminal window as well.
