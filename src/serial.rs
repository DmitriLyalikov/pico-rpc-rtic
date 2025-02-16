use crate::protocol::{ValidHostInterfaces,  host::{self, HostRequest, ValidInterfaces, ValidOps}};

use rp_pico::hal as hal;
// USB Device support 
use usb_device::{class_prelude::*};
// USB Communications Class Device support
use usbd_serial::SerialPort;

use core::{str, u32};
use core::str::SplitWhitespace;


// Helper function to ensure all data is written across the serial interface
#[inline(never)]
#[link_section = ".data.bar"] // Execute from IRAM
pub fn write_serial(serial: &mut SerialPort<'static, hal::usb::UsbBus>, buf: &str, block: bool) {
    let write_ptr = buf.as_bytes();

    // Because the buffer is of constant size and initialized to zero (0) we 
    //  add a test to determine the size that's really occupied by the str that we
    // want to send. From index zero to first byte that is as the zero byte value
    let mut index = 0;
    while index < write_ptr.len() && write_ptr[index] != 0 {
        index += 1;
    }
    let mut write_ptr = &write_ptr[0..index];

    while !write_ptr.is_empty() {
        match serial.write(write_ptr) {
            Ok(len) => write_ptr = &write_ptr[len..],
            // Meaning the USB write buffer is full
            Err(UsbError::WouldBlock) => {
                if !block {
                    break;
                }
            }
            // On error, just drop unwritten data
            Err(_) => break,
        }
    }
    let _ = serial.flush();
}

// Match the Serial Input commands to a hardware/software request
#[inline(never)]
#[link_section = ".data.bar"] // Execute from IRAM
pub fn match_usb_serial_buf( buf: &[u8; 64],
    serial: &mut SerialPort<'static, hal::usb::UsbBus> ) 
    -> Result<HostRequest<host::Unclean>, &'static str> {
    let buf = str::from_utf8(buf).unwrap();
    write_serial(serial, "\n\r", false);

    if slice_contains(buf, "menu") {
        print_menu(serial);
        Err("Ok")
    }
    else {
        write_serial(serial, "\n\r", false);
        message_parse_build(buf)
    }
}

pub fn print_menu(serial: &mut SerialPort<'static, hal::usb::UsbBus>){
    let mut _buf = [0u8; 273];
    // Create the Menu.
    let menu_str = "*****************\n\r
*  pico-bridge USB Serial Interface\n\r
*  Send system or device interface commands\n\r
*  Menu:\n\r
*  M / m - Print menu\n\r
*    - smi r phyAddr RegAddr\n\r
*    - smi w phyAddr RegAddr Data\n\r
*    - smi setclk frequency\n\r
*    - gpio set level\n\r 
*****************\n\r
Enter option: ";

    write_serial(serial, menu_str, true);
}

pub fn slice_contains(haystack: &str, needle: &str) -> bool {
    if haystack.len() < needle.len() {
        return false;
    }

    for i in 0..=(haystack.len() - needle.len()) {
        if &haystack[i..(i + needle.len())] == needle {
            return true;
        }
    }
    false
}

// Helper function that takes list of bytes and deconstructs
// into HostRequest fields. 
// NOTE: Preliminary behavior is to drop message and log to serial an invalid message
// if fields are missing or invalid
#[inline(never)]
#[link_section = ".data.bar"] // Execute from IRAM
pub fn message_parse_build<'input>(input: &'input str) 
    -> Result<HostRequest<host::Unclean>, &'static str>{
    let mut payload = [0u32; 4];

    // Split up the given string
    let mut hr = HostRequest::new();
    hr.set_host_config(ValidHostInterfaces::Serial);

    let words = |input: &'input str| -> SplitWhitespace<'input>  {input.split_whitespace()};
    let mut command = words(input);
    let command_count = command.clone().count();
    if command_count > 6 {
        return Err("Too many arguments\n\r")
    }
    // Match on the first word
    match command.next() {
        Some("smi" | "SMI") => {
            hr.set_interface(ValidInterfaces::SMI);
        }
        Some("cfg" | "CFG") => {
            hr.set_interface(ValidInterfaces::Config);
        }
        Some("gpio" | "GPIO") => {
            hr.set_interface(ValidInterfaces::GPIO);
        }
        Some("jtag" | "JTAG") => {
            hr.set_interface(ValidInterfaces::JTAG);
        }
        Some("spi" | "SPI") => {
            hr.set_interface(ValidInterfaces::SPI);
        }
        _ => {
            return Err("Invalid Interface\n\r")
        }
    }
    // Match on the second word. This should be an operation. If not log incorrect
    match command.next() {
        Some("r" | "R") => {
            hr.set_operation(ValidOps::Read);
        }
        Some("w" | "W") => {
            hr.set_operation(ValidOps::Write);
        }
        Some("smiset" | "SMISET") => {
            hr.set_operation(ValidOps::SmiSet);
        }
        _ => {
            return Err("Invalid Operation\n\r");
        }
    }
    let mut size: u8 = 0;
    while size < (command_count - 3) as u8 {
        let val = command.nth(0).unwrap();
            match bytes_to_number(val) {
                Ok(value) => {
                    payload[size as usize] = value;
                }
                Err(err) => {
                    return Err(err) 
                }
        }
        size+=1;
    }
    hr.set_size(size);
    hr.set_payload(payload);
    Ok(hr)
}

// Helper function to take &str in decimal or hex form
// and return u32.
// ie: s = "0xFF"  will return decimal value 255
#[inline(never)]
#[link_section = ".data.bar"] // Execute from IRAM
pub fn bytes_to_number(s: &str) -> Result<u32, &'static str> {
    let mut result: u32 = 0;
    // Check if the input is hex or decimal
    let mut chars = s.chars();
    if let Some(c) = chars.next() {
        if c != '0' || chars.next() != Some('x') {
            if '0' <= c && c <= '9' {
                result += c as u32 - '0' as u32;
                for c in chars {
                    let digit= match c {
                        '0'..='9' => c as u32 - '0' as u32,
                        _ => return Err("Invalid decimal character\n\r"),
                    };
                    if result >= 429_496_720 {
                        return Err("Integer number too large!\n\r")
                    }
                    result = result * 10 + digit;
                    
                }
                return Ok(result)
            }
            return Err("Not a hex or decimal string\n\r")
        }
    }
    if chars.clone().count() > 8 {
        return Err("Integer number too large!\n\r")
    }
    for c in chars {
        let digit =  match c {
            '0'..='9' => c as u32 - '0' as u32,
            'a'..='f' => c as u32 - 'a' as u32 + 10,
            'A'..='F' => c as u32 - 'A' as u32 + 10,
            _ => return Err("Invalid hex character\n\r"),
        };
        result = result * 16 + digit;
    }
    Ok(result)
}