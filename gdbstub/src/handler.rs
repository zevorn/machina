// GDB RSP command handler.
//
// Dispatches incoming GDB packets to the target backend
// and returns the response string.

use crate::protocol;

/// Reason the target stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopReason {
    /// Stopped by software breakpoint (SIGTRAP).
    Breakpoint,
    /// Stopped by watchpoint hit.
    Watchpoint { addr: u64, wtype: u8 },
    /// Stopped by single-step (SIGTRAP).
    Step,
    /// Stopped by GDB pause request (SIGTRAP).
    Pause,
    /// Target terminated.
    Terminated,
}

/// Trait for GDB target operations.
/// Implemented by the CPU/system layer.
pub trait GdbTarget: Send {
    fn read_registers(&self) -> Vec<u8>;
    fn write_registers(&mut self, _data: &[u8]) -> bool {
        false
    }
    fn read_register(&self, _reg: usize) -> Vec<u8>;
    fn write_register(&mut self, _reg: usize, _val: &[u8]) -> bool {
        false
    }
    fn read_memory(&self, addr: u64, len: usize) -> Vec<u8>;
    fn write_memory(&mut self, addr: u64, data: &[u8]) -> bool;
    /// type_: 0=sw, 1=hw, 2=write wp, 3=read wp,
    /// 4=access wp.
    fn set_breakpoint(&mut self, type_: u8, addr: u64, kind: u32) -> bool;
    fn remove_breakpoint(&mut self, type_: u8, addr: u64, kind: u32) -> bool;
    fn resume(&mut self);
    fn step(&mut self);
    fn get_pc(&self) -> u64;
    fn get_stop_reason(&self) -> StopReason;

    // -- Multi-vCPU --
    fn cpu_count(&self) -> usize {
        1
    }
    fn set_g_cpu(&mut self, _idx: usize) -> bool {
        true
    }
    fn set_c_cpu(&mut self, _idx: usize) -> bool {
        true
    }
    fn thread_alive(&self, _tid: usize) -> bool {
        true
    }
    fn stop_thread(&self) -> usize {
        1
    }

    // -- PhyMemMode --
    fn set_phy_mem_mode(&mut self, _enabled: bool) -> bool {
        false
    }
    fn phy_mem_mode(&self) -> bool {
        false
    }

    // -- Watchpoint hit info --
    fn take_watchpoint_hit(&mut self) -> Option<(u64, u8)> {
        None
    }
}

/// GDB command handler. Processes one packet at a time.
pub struct GdbHandler {
    no_ack: bool,
    attached: bool,
    target_xml: &'static str,
}

impl GdbHandler {
    pub fn new() -> Self {
        Self::with_target_xml(crate::target::RISCV64_TARGET_XML)
    }

    pub fn with_target_xml(xml: &'static str) -> Self {
        Self {
            no_ack: false,
            attached: true,
            target_xml: xml,
        }
    }

    pub fn handle(
        &mut self,
        packet: &str,
        target: &mut dyn GdbTarget,
    ) -> Option<String> {
        if packet == "\x03" {
            return Some(self.stop_reply(StopReason::Pause, target));
        }

        if packet.starts_with('v') {
            return self.handle_v_packet(packet, target);
        }

        if packet.starts_with('q') {
            let (name, args) = match packet.find(|c: char| !c.is_alphabetic()) {
                Some(i) => (&packet[..i], &packet[i..]),
                None => (packet, ""),
            };
            return Some(self.handle_query(name, args, target));
        }
        if packet.starts_with('Q') {
            let (name, args) = match packet.find(|c: char| !c.is_alphabetic()) {
                Some(i) => (&packet[..i], &packet[i..]),
                None => (packet, ""),
            };
            return Some(self.handle_set(name, args, target));
        }

        let (cmd, args) = match packet.chars().next() {
            Some('?') => ("?", &packet[1..]),
            Some(c) if c.is_ascii_uppercase() => (&packet[..1], &packet[1..]),
            Some(c) if c.is_ascii_lowercase() => (&packet[..1], &packet[1..]),
            _ => (packet, ""),
        };

        let resp = match cmd {
            "?" => self.handle_stop_reason(target),
            "g" => self.handle_read_registers(target),
            "G" => self.handle_write_registers(target, args),
            "p" => self.handle_read_register(target, args),
            "P" => self.handle_write_register(target, args),
            "m" => self.handle_read_memory(target, args),
            "M" => self.handle_write_memory(target, args),
            "X" => self.handle_write_memory_binary(target, args),
            "c" => return self.handle_continue(target),
            "s" => return self.handle_step(target),
            "Z" => self.handle_set_breakpoint(target, args),
            "z" => self.handle_remove_breakpoint(target, args),
            "D" => {
                self.attached = false;
                return None;
            }
            "k" => return None,
            "H" => self.handle_h_packet(target, args),
            "T" => self.handle_thread_alive(target, args),
            _ => String::new(),
        };

        Some(resp)
    }

    fn handle_stop_reason(&self, target: &mut dyn GdbTarget) -> String {
        self.stop_reply(target.get_stop_reason(), target)
    }

    fn stop_reply(&self, reason: StopReason, target: &dyn GdbTarget) -> String {
        let tid = target.stop_thread();
        match reason {
            StopReason::Breakpoint => {
                format!("T05thread:{:02x};swbreak:;", tid,)
            }
            StopReason::Watchpoint { addr, wtype } => {
                let prefix = match wtype {
                    1 => "rwatch",
                    2 => "awatch",
                    _ => "watch",
                };
                format!("T05thread:{:02x};{}:{:x};", tid, prefix, addr,)
            }
            StopReason::Step => {
                format!("T05thread:{:02x};", tid)
            }
            StopReason::Pause => {
                format!("T02thread:{:02x};", tid)
            }
            StopReason::Terminated => "W00".to_string(),
        }
    }

    fn handle_h_packet(
        &self,
        target: &mut dyn GdbTarget,
        args: &str,
    ) -> String {
        if args.is_empty() {
            return "OK".to_string();
        }
        let op = args.as_bytes()[0];
        let tid_str = &args[1..];
        // tid: -1 or 0 means "any", otherwise 1-based.
        let tid = if tid_str == "-1" || tid_str == "0" {
            0usize
        } else {
            protocol::parse_hex(tid_str) as usize
        };
        // Convert 1-based thread ID to 0-based index.
        let idx = if tid == 0 { 0 } else { tid - 1 };
        let ok = match op {
            b'g' => target.set_g_cpu(idx),
            b'c' => target.set_c_cpu(idx),
            _ => true,
        };
        if ok {
            "OK".to_string()
        } else {
            "E01".to_string()
        }
    }

    fn handle_thread_alive(
        &self,
        target: &mut dyn GdbTarget,
        args: &str,
    ) -> String {
        let tid = protocol::parse_hex(args) as usize;
        if target.thread_alive(tid) {
            "OK".to_string()
        } else {
            "E01".to_string()
        }
    }

    fn handle_read_registers(&self, target: &mut dyn GdbTarget) -> String {
        let data = target.read_registers();
        protocol::encode_hex_bytes(&data)
    }

    fn handle_write_registers(
        &self,
        target: &mut dyn GdbTarget,
        args: &str,
    ) -> String {
        match protocol::decode_hex_bytes(args) {
            Ok(data) => {
                if target.write_registers(&data) {
                    "OK".to_string()
                } else {
                    "E01".to_string()
                }
            }
            Err(_) => "E01".to_string(),
        }
    }

    fn handle_read_register(
        &self,
        target: &mut dyn GdbTarget,
        args: &str,
    ) -> String {
        let reg = protocol::parse_hex(args.trim_start_matches(':')) as usize;
        let data = target.read_register(reg);
        if data.is_empty() {
            "E00".to_string()
        } else {
            protocol::encode_hex_bytes(&data)
        }
    }

    fn handle_write_register(
        &self,
        target: &mut dyn GdbTarget,
        args: &str,
    ) -> String {
        let parts: Vec<&str> = args.splitn(2, '=').collect();
        if parts.len() != 2 {
            return "E01".to_string();
        }
        let reg = protocol::parse_hex(parts[0]) as usize;
        match protocol::decode_hex_bytes(parts[1]) {
            Ok(data) => {
                if target.write_register(reg, &data) {
                    "OK".to_string()
                } else {
                    "E01".to_string()
                }
            }
            Err(_) => "E01".to_string(),
        }
    }

    fn handle_read_memory(
        &self,
        target: &mut dyn GdbTarget,
        args: &str,
    ) -> String {
        let parts: Vec<&str> = args.splitn(2, ',').collect();
        if parts.len() != 2 {
            return "E01".to_string();
        }
        let addr = protocol::parse_hex(parts[0]);
        let len = protocol::parse_hex(parts[1]) as usize;
        let data = target.read_memory(addr, len);
        protocol::encode_hex_bytes(&data)
    }

    fn handle_write_memory(
        &self,
        target: &mut dyn GdbTarget,
        args: &str,
    ) -> String {
        let colon = match args.find(':') {
            Some(i) => i,
            None => return "E01".to_string(),
        };
        let header = &args[..colon];
        let data_hex = &args[colon + 1..];
        let parts: Vec<&str> = header.splitn(2, ',').collect();
        if parts.len() != 2 {
            return "E01".to_string();
        }
        let addr = protocol::parse_hex(parts[0]);
        match protocol::decode_hex_bytes(data_hex) {
            Ok(data) => {
                if target.write_memory(addr, &data) {
                    "OK".to_string()
                } else {
                    "E01".to_string()
                }
            }
            Err(_) => "E01".to_string(),
        }
    }

    fn handle_write_memory_binary(
        &self,
        target: &mut dyn GdbTarget,
        args: &str,
    ) -> String {
        let colon = match args.find(':') {
            Some(i) => i,
            None => return "E01".to_string(),
        };
        let header = &args[..colon];
        let data = &args[colon + 1..];
        let parts: Vec<&str> = header.splitn(2, ',').collect();
        if parts.len() != 2 {
            return "E01".to_string();
        }
        let addr = protocol::parse_hex(parts[0]);
        let unescaped = unescape_binary(data.as_bytes());
        if target.write_memory(addr, &unescaped) {
            "OK".to_string()
        } else {
            "E01".to_string()
        }
    }

    fn handle_continue(
        &mut self,
        target: &mut dyn GdbTarget,
    ) -> Option<String> {
        target.resume();
        Some(self.stop_reply(target.get_stop_reason(), target))
    }

    fn handle_step(&mut self, target: &mut dyn GdbTarget) -> Option<String> {
        target.step();
        Some(self.stop_reply(StopReason::Step, target))
    }

    fn handle_set_breakpoint(
        &self,
        target: &mut dyn GdbTarget,
        args: &str,
    ) -> String {
        let parts: Vec<&str> = args.splitn(3, ',').collect();
        if parts.len() < 2 {
            return "E01".to_string();
        }
        let type_ = parts[0].parse::<u8>().unwrap_or(0);
        let addr = protocol::parse_hex(parts[1]);
        let kind = parts
            .get(2)
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(4);
        if target.set_breakpoint(type_, addr, kind) {
            "OK".to_string()
        } else {
            String::new()
        }
    }

    fn handle_remove_breakpoint(
        &self,
        target: &mut dyn GdbTarget,
        args: &str,
    ) -> String {
        let parts: Vec<&str> = args.splitn(3, ',').collect();
        if parts.len() < 2 {
            return "E01".to_string();
        }
        let type_ = parts[0].parse::<u8>().unwrap_or(0);
        let addr = protocol::parse_hex(parts[1]);
        let kind = parts
            .get(2)
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(4);
        if target.remove_breakpoint(type_, addr, kind) {
            "OK".to_string()
        } else {
            String::new()
        }
    }

    fn handle_query(
        &mut self,
        name: &str,
        args: &str,
        target: &mut dyn GdbTarget,
    ) -> String {
        match name {
            "qSupported" => "multiprocess+;\
                 vContSupported+;\
                 QStartNoAckMode+;\
                 PacketSize=4000;\
                 qXfer:features:read+;\
                 hwbreak+;\
                 swbreak+"
                .to_string(),
            "qAttached" => if self.attached { "1" } else { "0" }.to_string(),
            "qC" => "QC01".to_string(),
            "qfThreadInfo" => {
                let n = target.cpu_count();
                let mut r = String::from("m");
                for i in 1..=n {
                    if i > 1 {
                        r.push(',');
                    }
                    r.push_str(&format!("{:02x}", i));
                }
                r
            }
            "qsThreadInfo" => "l".to_string(),
            "qOffsets" => String::new(),
            _ if name.starts_with("qSymbol") => "OK".to_string(),
            _ if name.starts_with("qThreadExtraInfo") => {
                // Extract thread ID from args.
                let tid_str = args.trim_start_matches(',');
                let tid = protocol::parse_hex(tid_str) as usize;
                let desc = format!("CPU {}", tid.saturating_sub(1),);
                protocol::encode_hex_bytes(desc.as_bytes())
            }
            _ if name.starts_with("qRcmd") => self.handle_qrcmd(args, target),
            _ if name.starts_with("qqemu") => {
                self.handle_qemu_query(name, target)
            }
            _ if name.starts_with("qXfer") => self.handle_qxfer(args),
            _ => String::new(),
        }
    }

    fn handle_qrcmd(&self, args: &str, _target: &mut dyn GdbTarget) -> String {
        let hex = args.trim_start_matches(',');
        let cmd_bytes = match protocol::decode_hex_bytes(hex) {
            Ok(b) => b,
            Err(_) => return encode_o_packet("Error: invalid hex\n"),
        };
        let cmd = String::from_utf8_lossy(&cmd_bytes);
        // Return the command as an O packet echo
        // (monitor passthrough placeholder).
        let output = format!("Unknown monitor command: {}\n", cmd);
        encode_o_packet(&output)
    }

    fn handle_qemu_query(
        &self,
        name: &str,
        target: &mut dyn GdbTarget,
    ) -> String {
        if name == "qqemuPhyMemMode" || name.contains("PhyMemMode") {
            let mode = target.phy_mem_mode();
            return if mode {
                "1".to_string()
            } else {
                "0".to_string()
            };
        }
        String::new()
    }

    fn handle_qxfer(&self, args: &str) -> String {
        let parts: Vec<&str> = args.split(':').collect();
        if parts.len() < 5 {
            return String::new();
        }
        let object = parts[1];
        let action = parts[2];
        let annex = parts[3];
        let range = parts[4];

        if object != "features" || action != "read" {
            return String::new();
        }
        if annex != "target.xml" {
            return String::new();
        }

        let range_parts: Vec<&str> = range.split(',').collect();
        if range_parts.len() != 2 {
            return String::new();
        }
        let offset = protocol::parse_hex(range_parts[0]) as usize;
        let length = protocol::parse_hex(range_parts[1]) as usize;

        let xml = self.target_xml;
        if offset >= xml.len() {
            return "l".to_string();
        }
        let end = (offset + length).min(xml.len());
        let data = &xml.as_bytes()[offset..end];
        let mut resp = String::with_capacity(data.len() + 1);
        if offset + data.len() < xml.len() {
            resp.push('m');
        } else {
            resp.push('l');
        }
        resp.push_str(std::str::from_utf8(data).unwrap_or(""));
        resp
    }

    fn handle_set(
        &mut self,
        name: &str,
        args: &str,
        target: &mut dyn GdbTarget,
    ) -> String {
        match name {
            "QStartNoAckMode" => {
                self.no_ack = true;
                "OK".to_string()
            }
            _ if name.contains("PhyMemMode") => {
                let val_str = args.trim_start_matches(':');
                match val_str {
                    "0" => {
                        target.set_phy_mem_mode(false);
                        "OK".to_string()
                    }
                    "1" => {
                        target.set_phy_mem_mode(true);
                        "OK".to_string()
                    }
                    _ => "E01".to_string(),
                }
            }
            _ => String::new(),
        }
    }

    fn handle_v_packet(
        &mut self,
        packet: &str,
        target: &mut dyn GdbTarget,
    ) -> Option<String> {
        if packet == "vCont?" {
            return Some("vCont;c;C;s;S;t".to_string());
        }
        if let Some(rest) = packet.strip_prefix("vCont") {
            if rest.is_empty() || rest == ";" {
                return Some("OK".to_string());
            }
            return self.handle_v_cont(rest, target);
        }
        if packet.starts_with("vMustReplyEmpty") {
            return Some(String::new());
        }
        Some(String::new())
    }

    fn handle_v_cont(
        &mut self,
        args: &str,
        target: &mut dyn GdbTarget,
    ) -> Option<String> {
        // vCont;action[:thread];action[:thread]...
        let actions: Vec<&str> =
            args.trim_start_matches(';').split(';').collect();
        let action = actions.first()?;

        let (cmd, _thread): (&str, &str) = match action.find(':') {
            Some(i) => (&action[..i], &action[i + 1..]),
            None => (*action, ""),
        };

        match cmd {
            "c" | "C" => self.handle_continue(target),
            "s" | "S" => self.handle_step(target),
            _ => Some(String::new()),
        }
    }

    pub fn no_ack(&self) -> bool {
        self.no_ack
    }
}

/// Encode a string as a GDB O packet (hex-encoded
/// console output).
fn encode_o_packet(s: &str) -> String {
    let mut out = String::with_capacity(1 + s.len() * 2);
    out.push('O');
    out.push_str(&protocol::encode_hex_bytes(s.as_bytes()));
    out
}

/// Unescape GDB binary data (}XOR escaping).
fn unescape_binary(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        if data[i] == b'}' && i + 1 < data.len() {
            out.push(data[i + 1] ^ 0x20);
            i += 2;
        } else {
            out.push(data[i]);
            i += 1;
        }
    }
    out
}

impl Default for GdbHandler {
    fn default() -> Self {
        Self::new()
    }
}
