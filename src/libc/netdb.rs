/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0.
 * If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `netdb.h` — host/service name resolution stubs.

use crate::dyld::{ConstantExports, FunctionExports, HostConstant};
use crate::export_c_func;
use crate::libc::sys::socket::{sockaddr, AF_INET, SOCK_DGRAM, SOCK_STREAM};
use crate::mem::{guest_size_of, ConstPtr, ConstVoidPtr, MutPtr, Ptr, SafeRead};
use crate::Environment;
use std::net::ToSocketAddrs;

const AI_PASSIVE: i32 = 0x1;
const AI_CANONNAME: i32 = 0x2;
const AI_NUMERICHOST: i32 = 0x4;
const AI_NUMERICSERV: i32 = 0x8;
const AI_V4MAPPED: i32 = 0x800;
const AI_ALL: i32 = 0x100;
const AI_ADDRCONFIG: i32 = 0x400;

pub const IPPROTO_TCP: i32 = 6;
pub const IPPROTO_UDP: i32 = 17;
const EAI_AGAIN: i32 = 2;
const EAI_FAIL: i32 = 4;
const EAI_FAMILY: i32 = 5;
const EAI_SERVICE: i32 = 8;
const EAI_NONAME: i32 = 8;
const EAI_MEMORY: i32 = 6;
const EAI_SYSTEM: i32 = 11;
const EAI_OVERFLOW: i32 = 14;
const HOST_NOT_FOUND: i32 = 1;
const TRY_AGAIN: i32 = 2;
const NO_RECOVERY: i32 = 3;
const NO_DATA: i32 = 4;
const NI_MAXHOST: u32 = 1025;
const NI_MAXSERV: u32 = 32;

const NI_NUMERICHOST: i32 = 0x01;
const NI_NUMERICSERV: i32 = 0x02;
const NI_NAMEREQD: i32 = 0x04;
const NI_NOFQDN: i32 = 0x08;
const NI_DGRAM: i32 = 0x10;

#[allow(non_camel_case_types)]
pub type socklen_t = u32;
// h_errno values stored in libc state.
pub const H_ERRNO_SUCCESS: i32 = 0;
pub const H_ERRNO_HOST_NOT_FOUND: i32 = HOST_NOT_FOUND;
pub const H_ERRNO_TRY_AGAIN: i32 = TRY_AGAIN;
pub const H_ERRNO_NO_RECOVERY: i32 = NO_RECOVERY;
pub const H_ERRNO_NO_DATA: i32 = NO_DATA;
// AF_INET in network byte order for in_addr.
const AF_INET_NBO: u16 = (AF_INET as u16).rotate_right(8);

#[derive(Default)]
pub struct State {
    /// Raw guest address of the persistent `hostent` block.
    pub dummy_hostent_ptr: u32,
    /// Cached host-side mirror of the guest `h_errno` cell. Kept in sync
    /// with `h_errno_cell` so that Rust code can read the most-recently-set
    /// value without going through `env.mem`.
    pub h_errno: i32,
    /// Address (in guest memory) of the persistent `int h_errno;` cell
    /// exported by libSystem. Apple's `netdb.h` declares `h_errno` as
    /// `extern int h_errno;` and ships a real storage slot in libsystem;
    /// `__h_errno_location()` returns the same pointer on every call.
    /// We lazily allocate one 4-byte cell on first access and re-use it
    /// for both the non-lazy `_h_errno` symbol and `__h_errno_location`.
    pub h_errno_cell: Option<MutPtr<i32>>,
}

/// Returns the guest-memory pointer to the persistent `h_errno` cell,
/// allocating it on first use. Apple's libsystem exposes `h_errno` as
/// a real global; this helper guarantees we hand out the *same* address
/// every time, which is required by code that compares the pointer
/// returned from `__h_errno_location()` across calls (and by anything
/// that resolves the non-lazy `_h_errno` symbol).
pub fn h_errno_ptr(env: &mut Environment) -> MutPtr<i32> {
    if let Some(ptr) = env.libc_state.netdb.h_errno_cell {
        return ptr;
    }
    let ptr: MutPtr<i32> = env.mem.alloc(guest_size_of::<i32>()).cast();
    env.mem.write(ptr, env.libc_state.netdb.h_errno);
    env.libc_state.netdb.h_errno_cell = Some(ptr);
    ptr
}

/// Sets `h_errno` per Apple's `netdb.h` semantics: updates both the
/// host-side mirror and the persistent guest cell so that guest code
/// reading the cell directly (e.g. via `_h_errno`) sees the new value.
pub fn set_h_errno(env: &mut Environment, value: i32) {
    env.libc_state.netdb.h_errno = value;
    if let Some(ptr) = env.libc_state.netdb.h_errno_cell {
        env.mem.write(ptr, value);
    }
}

/// Real in-memory layout of `struct hostent` on 32-bit iOS/macOS.
/// All pointer fields are 4-byte guest pointers.
#[derive(Copy, Clone, Debug)]
#[repr(C, packed)]
struct hostent_guest {
    h_name: MutPtr<u8>,              // canonical name
    h_aliases: MutPtr<MutPtr<u8>>,   // NULL-terminated alias list
    h_addrtype: i32,                 // AF_INET
    h_length: i32,                   // 4 for IPv4
    h_addr_list: MutPtr<MutPtr<u8>>, // NULL-terminated address list
}
unsafe impl SafeRead for hostent_guest {}

/// `struct servent` layout.
#[derive(Copy, Clone, Debug)]
#[repr(C, packed)]
struct servent_guest {
    s_name: MutPtr<u8>,
    s_aliases: MutPtr<MutPtr<u8>>,
    s_port: i32, // port in network byte order
    s_proto: MutPtr<u8>,
}
unsafe impl SafeRead for servent_guest {}

#[derive(Copy, Clone, Debug)]
#[repr(C, packed)]
#[allow(non_camel_case_types)]
pub struct addrinfo {
    ai_flags: i32,
    ai_family: i32,
    ai_socktype: i32,
    ai_protocol: i32,
    ai_addrlen: socklen_t,
    ai_canonname: MutPtr<u8>,
    ai_addr: MutPtr<sockaddr>,
    ai_next: MutPtr<addrinfo>,
}
unsafe impl SafeRead for addrinfo {}

// MARK: - Internal helpers

/// Parse a dotted-decimal IPv4 address string.
fn parse_ipv4(s: &str) -> Option<[u8; 4]> {
    s.parse::<std::net::Ipv4Addr>().ok().map(|a| a.octets())
}

/// Well-known service name → port (host byte order).
fn service_port(name: &str) -> Option<u16> {
    match name {
        "http" => Some(80),
        "https" => Some(443),
        "ftp" => Some(21),
        "ftp-data" => Some(20),
        "ssh" => Some(22),
        "telnet" => Some(23),
        "smtp" => Some(25),
        "dns" => Some(53),
        "pop3" => Some(110),
        "nntp" => Some(119),
        "imap" => Some(143),
        "imap2" => Some(143),
        "ldap" => Some(389),
        // Убрано дублирующееся значение "https" => Some(443),
        "smtps" => Some(465),
        "imaps" => Some(993),
        "pop3s" => Some(995),
        "ntp" => Some(123),
        "snmp" => Some(161),
        _ => None,
    }
}

/// Port (host byte order) → well-known service name.
fn port_service(port: u16) -> Option<&'static str> {
    match port {
        20 => Some("ftp-data"),
        21 => Some("ftp"),
        22 => Some("ssh"),
        23 => Some("telnet"),
        25 => Some("smtp"),
        53 => Some("dns"),
        80 => Some("http"),
        110 => Some("pop3"),
        119 => Some("nntp"),
        123 => Some("ntp"),
        143 => Some("imap"),
        161 => Some("snmp"),
        443 => Some("https"),
        465 => Some("smtps"),
        993 => Some("imaps"),
        995 => Some("pop3s"),
        _ => None,
    }
}

/// Allocate a guest `hostent` for `ip_octets` with the given `canonical_name`.
/// Returns the pointer to the struct; caller must not free (caller manages
//lifetime).
fn alloc_hostent(env: &mut Environment, ip_octets: [u8; 4], canonical_name: &str) -> u32 {
    // Layout in guest memory:
    //   hostent_guest  (5 × 4 = 20 bytes)
    //   canonical name (len+1 bytes)
    //   ip bytes       (4 bytes)
    //   h_addr_list[0] = ptr to ip bytes   (4 bytes)
    //   h_addr_list[1] = NULL              (4 bytes)
    //   h_aliases[0]   = NULL              (4 bytes)   (empty alias list)

    let name_bytes = canonical_name.as_bytes();
    let name_len = name_bytes.len() as u32 + 1;
    let total = guest_size_of::<hostent_guest>()
        + name_len
        + 4   // ip bytes
        + 4   // addr_list[0]
        + 4   // addr_list[1] (NULL)
        + 4; // aliases[0]  (NULL)

    let block: MutPtr<u8> = env.mem.alloc(total).cast();
    let base = block.to_bits();

    // Offsets.
    let name_off = guest_size_of::<hostent_guest>();
    let ip_off = name_off + name_len;
    let addrlist_off = ip_off + 4;
    let aliases_off = addrlist_off + 8;

    // Write name.
    let name_ptr: MutPtr<u8> = MutPtr::from_bits(base + name_off);
    for (i, &b) in name_bytes.iter().enumerate() {
        env.mem.write(name_ptr + i as u32, b);
    }
    env.mem.write(name_ptr + name_bytes.len() as u32, 0u8);

    // Write IP bytes.
    let ip_ptr: MutPtr<u8> = MutPtr::from_bits(base + ip_off);
    for (i, &b) in ip_octets.iter().enumerate() {
        env.mem.write(ip_ptr + i as u32, b);
    }

    // Write addr_list: [ptr_to_ip, NULL].
    let addrlist_ptr: MutPtr<MutPtr<u8>> = MutPtr::from_bits(base + addrlist_off);
    env.mem.write(addrlist_ptr, ip_ptr);
    env.mem.write(addrlist_ptr + 1u32, MutPtr::null());

    // Write aliases: [NULL].
    let aliases_ptr: MutPtr<MutPtr<u8>> = MutPtr::from_bits(base + aliases_off);
    env.mem.write(aliases_ptr, MutPtr::null());

    // Write the hostent_guest header.
    let he = hostent_guest {
        h_name: name_ptr,
        h_aliases: aliases_ptr,
        h_addrtype: AF_INET,
        h_length: 4,
        h_addr_list: addrlist_ptr,
    };
    let he_ptr: MutPtr<hostent_guest> = MutPtr::from_bits(base);
    env.mem.write(he_ptr, he);

    base
}

// MARK: - gethostbyname / gethostbyaddr

/// Resolve a hostname to an IPv4 address by asking the host OS's resolver.
///
/// Used by `gethostbyname` and `getaddrinfo`. Returns `None` when the lookup
/// fails or when the hostname has no IPv4 record. This is a blocking call.
fn resolve_hostname_ipv4(hostname: &str) -> Option<[u8; 4]> {
    // Port 0 is just a placeholder; we only care about the resolved address.
    let addrs = (hostname, 0u16).to_socket_addrs().ok()?;
    for addr in addrs {
        if let std::net::SocketAddr::V4(v4) = addr {
            return Some(v4.ip().octets());
        }
    }
    None
}

fn gethostbyname(env: &mut Environment, name: ConstPtr<u8>) -> MutPtr<u8> {
    set_h_errno(env, H_ERRNO_SUCCESS);
    let hostname = if name.is_null() {
        "localhost".to_string()
    } else {
        env.mem.cstr_at_utf8(name).unwrap_or_default().to_owned()
    };
    log_dbg!("gethostbyname(\"{}\")", hostname);

    // Resolve: try dotted-decimal first, then well-known names, then real DNS.
    let ip_octets: [u8; 4] = if let Some(octets) = parse_ipv4(&hostname) {
        octets
    } else {
        match hostname.as_str() {
            "localhost" | "loopback" | "touchHLE" => [127, 0, 0, 1],
            "broadcasthost" => [255, 255, 255, 255],
            _ => {
                if !env.options.network_access {
                    log!(
                        "gethostbyname(\"{}\"): network disabled -> HOST_NOT_FOUND",
                        hostname
                    );
                    set_h_errno(env, H_ERRNO_HOST_NOT_FOUND);
                    return MutPtr::null();
                }
                // Ask the host OS resolver for an A record.
                match resolve_hostname_ipv4(&hostname) {
                    Some(octets) => {
                        log!(
                            "gethostbyname(\"{}\"): host resolver -> {}.{}.{}.{}",
                            hostname,
                            octets[0],
                            octets[1],
                            octets[2],
                            octets[3]
                        );
                        octets
                    }
                    None => {
                        log!(
                            "gethostbyname(\"{}\"): host resolver failed -> HOST_NOT_FOUND",
                            hostname
                        );
                        set_h_errno(env, H_ERRNO_HOST_NOT_FOUND);
                        return MutPtr::null();
                    }
                }
            }
        }
    };
    // Free previous hostent if any.
    if env.libc_state.netdb.dummy_hostent_ptr != 0 {
        let old: MutPtr<u8> = MutPtr::from_bits(env.libc_state.netdb.dummy_hostent_ptr);
        env.mem.free(old.cast());
    }

    let ptr = alloc_hostent(env, ip_octets, &hostname);
    env.libc_state.netdb.dummy_hostent_ptr = ptr;
    log_dbg!(
        "gethostbyname(\"{}\") -> {:?} at 0x{:08x}",
        hostname,
        ip_octets,
        ptr
    );
    MutPtr::from_bits(ptr)
}

fn gethostbyname2(env: &mut Environment, name: ConstPtr<u8>, af: i32) -> MutPtr<u8> {
    if af != AF_INET {
        log!("gethostbyname2: unsupported address family {}", af);
        set_h_errno(env, H_ERRNO_NO_RECOVERY);
        return MutPtr::null();
    }
    gethostbyname(env, name)
}

fn gethostbyaddr(
    env: &mut Environment,
    addr: ConstPtr<u8>,
    len: socklen_t,
    type_: i32,
) -> MutPtr<u8> {
    set_h_errno(env, H_ERRNO_SUCCESS);
    if type_ != AF_INET || len < 4 {
        log!("gethostbyaddr: unsupported family {} or len {}", type_, len);
        set_h_errno(env, H_ERRNO_NO_RECOVERY);
        return MutPtr::null();
    }

    let octets: [u8; 4] = [
        env.mem.read(addr),
        env.mem.read(addr + 1u32),
        env.mem.read(addr + 2u32),
        env.mem.read(addr + 3u32),
    ];
    let dotted = format!("{}.{}.{}.{}", octets[0], octets[1], octets[2], octets[3]);
    log_dbg!("gethostbyaddr({}) -> returning dotted form", dotted);
    if env.libc_state.netdb.dummy_hostent_ptr != 0 {
        let old: MutPtr<u8> = MutPtr::from_bits(env.libc_state.netdb.dummy_hostent_ptr);
        env.mem.free(old.cast());
    }

    let ptr = alloc_hostent(env, octets, &dotted);
    env.libc_state.netdb.dummy_hostent_ptr = ptr;
    MutPtr::from_bits(ptr)
}

// MARK: - getservbyname / getservbyport

fn getservbyname(env: &mut Environment, name: ConstPtr<u8>, proto: ConstPtr<u8>) -> MutPtr<u8> {
    let name_str = env.mem.cstr_at_utf8(name).unwrap_or_default().to_owned();
    let port = match service_port(&name_str) {
        Some(p) => p,
        None => {
            log!("getservbyname(\"{}\"): unknown service", name_str);
            return MutPtr::null();
        }
    };

    let proto_str = if proto.is_null() {
        "tcp".to_string()
    } else {
        env.mem.cstr_at_utf8(proto).unwrap_or_default().to_owned()
    };
    // port in network byte order (big-endian).
    let port_nbo = port.to_be() as i32;
    alloc_servent(env, &name_str, port_nbo, &proto_str)
}

fn getservbyport(env: &mut Environment, port_nbo: i32, proto: ConstPtr<u8>) -> MutPtr<u8> {
    let port_hbo = u16::from_be((port_nbo as u16).to_be());
    let name = match port_service(port_hbo) {
        Some(n) => n,
        None => {
            log!("getservbyport({}): unknown port", port_hbo);
            return MutPtr::null();
        }
    };

    let proto_str = if proto.is_null() {
        "tcp".to_string()
    } else {
        env.mem.cstr_at_utf8(proto).unwrap_or_default().to_owned()
    };
    alloc_servent(env, name, port_nbo, &proto_str)
}

fn alloc_servent(env: &mut Environment, name: &str, port_nbo: i32, proto: &str) -> MutPtr<u8> {
    let name_bytes = name.as_bytes();
    let proto_bytes = proto.as_bytes();
    let total = guest_size_of::<servent_guest>()
        + name_bytes.len() as u32
        + 1
        + proto_bytes.len() as u32
        + 1
        + 4; // aliases[0] = NULL

    let block: MutPtr<u8> = env.mem.alloc(total).cast();
    let base = block.to_bits();
    let name_off = guest_size_of::<servent_guest>();
    let proto_off = name_off + name_bytes.len() as u32 + 1;
    let aliases_off = proto_off + proto_bytes.len() as u32 + 1;

    let name_ptr: MutPtr<u8> = MutPtr::from_bits(base + name_off);
    let proto_ptr: MutPtr<u8> = MutPtr::from_bits(base + proto_off);
    let aliases_ptr: MutPtr<MutPtr<u8>> = MutPtr::from_bits(base + aliases_off);
    for (i, &b) in name_bytes.iter().enumerate() {
        env.mem.write(name_ptr + i as u32, b);
    }
    env.mem.write(name_ptr + name_bytes.len() as u32, 0u8);
    for (i, &b) in proto_bytes.iter().enumerate() {
        env.mem.write(proto_ptr + i as u32, b);
    }
    env.mem.write(proto_ptr + proto_bytes.len() as u32, 0u8);
    env.mem.write(aliases_ptr, MutPtr::<u8>::null());
    let sv = servent_guest {
        s_name: name_ptr,
        s_aliases: aliases_ptr,
        s_port: port_nbo,
        s_proto: proto_ptr,
    };
    env.mem.write(MutPtr::<servent_guest>::from_bits(base), sv);
    block
}

// MARK: - getaddrinfo / freeaddrinfo

fn getaddrinfo(
    env: &mut Environment,
    node_name: MutPtr<u8>,
    serv_name: MutPtr<u8>,
    hints: ConstPtr<addrinfo>,
    res: MutPtr<MutPtr<addrinfo>>,
) -> i32 {
    if !env.options.network_access && !node_name.is_null() {
        let hn = env
            .mem
            .cstr_at_utf8(node_name.cast_const())
            .unwrap_or_default()
            .to_owned();
        // Allow localhost even without network access.
        if hn != "localhost" && hn != "127.0.0.1" && hn != "touchHLE" && parse_ipv4(&hn).is_none() {
            log_dbg!("getaddrinfo: network disabled (node={}) -> EAI_FAIL", hn);
            return EAI_FAIL;
        }
    }

    let hint = if hints.is_null() {
        addrinfo {
            ai_flags: 0,
            ai_family: 0,
            ai_socktype: 0,
            ai_protocol: 0,
            ai_addrlen: 0,
            ai_canonname: Ptr::null(),
            ai_addr: Ptr::null(),
            ai_next: Ptr::null(),
        }
    } else {
        env.mem.read(hints)
    };
    let ai_flags = hint.ai_flags;
    let ai_family = hint.ai_family;
    let ai_socktype = hint.ai_socktype;
    let ai_protocol = hint.ai_protocol;

    if ai_family != AF_INET && ai_family != 0 {
        log!(
            "getaddrinfo: unsupported ai_family {} -> EAI_FAMILY",
            ai_family
        );
        return EAI_FAMILY;
    }
    if ai_socktype != 0 && ai_socktype != SOCK_STREAM && ai_socktype != SOCK_DGRAM {
        log!(
            "getaddrinfo: unsupported ai_socktype {} -> EAI_SERVICE",
            ai_socktype
        );
        return EAI_SERVICE;
    }
    if ai_protocol != 0 && ai_protocol != IPPROTO_TCP && ai_protocol != IPPROTO_UDP {
        log!(
            "getaddrinfo: unsupported ai_protocol {} -> EAI_FAIL",
            ai_protocol
        );
        return EAI_FAIL;
    }

    let ip_octets: [u8; 4] = if node_name.is_null() {
        if ai_flags & AI_PASSIVE != 0 {
            [0, 0, 0, 0]
        } else {
            [127, 0, 0, 1]
        }
    } else {
        let hostname = env
            .mem
            .cstr_at_utf8(node_name.cast_const())
            .unwrap_or_default()
            .to_owned();
        if let Some(octets) = parse_ipv4(&hostname) {
            octets
        } else {
            match hostname.as_str() {
                "localhost" | "loopback" | "touchHLE" => [127, 0, 0, 1],
                _ => {
                    if !env.options.network_access {
                        log!(
                            "getaddrinfo: network disabled (node={}) -> EAI_FAIL",
                            hostname
                        );
                        return EAI_FAIL;
                    }
                    match resolve_hostname_ipv4(&hostname) {
                        Some(octets) => {
                            log!(
                                "getaddrinfo(\"{}\"): host resolver -> {}.{}.{}.{}",
                                hostname,
                                octets[0],
                                octets[1],
                                octets[2],
                                octets[3]
                            );
                            octets
                        }
                        None => {
                            log!(
                                "getaddrinfo: hostname \"{}\" not resolvable -> EAI_FAIL",
                                hostname
                            );
                            return EAI_FAIL;
                        }
                    }
                }
            }
        }
    };
    let port: u16 = if serv_name.is_null() {
        0
    } else {
        let svc = env
            .mem
            .cstr_at_utf8(serv_name.cast_const())
            .unwrap_or_default()
            .to_owned();
        if let Ok(p) = svc.parse::<u16>() {
            p
        } else {
            match service_port(&svc) {
                Some(p) => p,
                None => {
                    log!(
                        "getaddrinfo: named service \"{}\" not supported -> EAI_SERVICE",
                        svc
                    );
                    return EAI_SERVICE;
                }
            }
        }
    };
    log_dbg!("getaddrinfo: ip={:?} port={}", ip_octets, port);

    let addr_ptr = env
        .mem
        .alloc_and_write(sockaddr::from_ipv4_parts(ip_octets, port));
    let result = addrinfo {
        ai_flags,
        ai_family: AF_INET,
        ai_socktype: if ai_socktype != 0 {
            ai_socktype
        } else {
            SOCK_STREAM
        },
        ai_protocol: if ai_protocol != 0 {
            ai_protocol
        } else {
            IPPROTO_TCP
        },
        ai_addrlen: guest_size_of::<sockaddr>(),
        ai_canonname: Ptr::null(),
        ai_addr: addr_ptr,
        ai_next: Ptr::null(),
    };
    let result_ptr = env.mem.alloc_and_write(result);
    if !res.is_null() {
        env.mem.write(res, result_ptr);
    }
    0
}

fn freeaddrinfo(env: &mut Environment, ai: MutPtr<addrinfo>) {
    if ai.is_null() {
        return;
    }
    let mut cur = ai;
    while !cur.is_null() {
        let node = env.mem.read(cur);
        let next = node.ai_next;
        if !node.ai_addr.is_null() {
            env.mem.free(node.ai_addr.cast());
        }
        if !node.ai_canonname.is_null() {
            env.mem.free(node.ai_canonname.cast());
        }
        env.mem.free(cur.cast());
        cur = next;
    }
}

// MARK: - getnameinfo

fn getnameinfo(
    env: &mut Environment,
    sa: ConstPtr<sockaddr>,
    salen: socklen_t,
    host: MutPtr<u8>,
    hostlen: u32,
    serv: MutPtr<u8>,
    servlen: u32,
    flags: i32,
) -> i32 {
    if sa.is_null() || salen < guest_size_of::<sockaddr>() {
        return EAI_FAIL;
    }

    // ИСПРАВЛЕНИЕ ЗДЕСЬ: Прямое чтение из гостевой памяти по смещениям
    // (AF_INET/sockaddr_in layout)
    let sa_ptr = sa.cast::<u8>();
    let port = u16::from_be_bytes([env.mem.read(sa_ptr + 2u32), env.mem.read(sa_ptr + 3u32)]);
    let octets: [u8; 4] = [
        env.mem.read(sa_ptr + 4u32),
        env.mem.read(sa_ptr + 5u32),
        env.mem.read(sa_ptr + 6u32),
        env.mem.read(sa_ptr + 7u32),
    ];

    if !host.is_null() && hostlen > 0 {
        let dotted = format!("{}.{}.{}.{}\0", octets[0], octets[1], octets[2], octets[3]);
        let bytes = dotted.as_bytes();
        let copy_len = bytes.len().min(hostlen as usize);
        for (i, &b) in bytes[..copy_len].iter().enumerate() {
            env.mem.write(host + i as u32, b);
        }
        // Ensure null termination.
        env.mem.write(host + (copy_len - 1) as u32, 0u8);
    }

    if !serv.is_null() && servlen > 0 {
        let svc_str = if flags & NI_NUMERICSERV != 0 {
            format!("{}\0", port)
        } else {
            match port_service(port) {
                Some(name) => format!("{}\0", name),
                None => format!("{}\0", port),
            }
        };
        let bytes = svc_str.as_bytes();
        let copy_len = bytes.len().min(servlen as usize);
        for (i, &b) in bytes[..copy_len].iter().enumerate() {
            env.mem.write(serv + i as u32, b);
        }
        env.mem.write(serv + (copy_len - 1) as u32, 0u8);
    }

    0
}

// MARK: - h_errno

fn __h_errno_location(env: &mut Environment) -> MutPtr<i32> {
    // Apple's `netdb.h` (`__h_errno_location` is the thread-local accessor
    // used by the macro `#define h_errno (*__h_errno_location())`) must
    // return a stable pointer to the *single* `h_errno` cell so that
    // `&h_errno` is well-defined and the value survives across calls.
    // Allocating a fresh cell each call — as a previous revision did —
    // breaks `errno`-style code (`*__h_errno_location() = 0;`) because
    // the next read sees a stale address. Funnel everyone through
    // `h_errno_ptr` which allocates once and caches in `State`.
    h_errno_ptr(env)
}

// MARK: - gai_strerror

fn gai_strerror(env: &mut Environment, ecode: i32) -> ConstPtr<u8> {
    let msg: &[u8] = match ecode {
        0 => b"Success\0",
        EAI_AGAIN => b"Temporary failure in name resolution\0",
        EAI_FAIL => b"Non-recoverable failure in name resolution\0",
        EAI_FAMILY => b"ai_family not supported\0",
        EAI_SERVICE => b"Servname not supported for ai_socktype\0",
        EAI_MEMORY => b"Memory allocation failure\0",
        EAI_OVERFLOW => b"Argument buffer overflow\0",
        EAI_SYSTEM => b"System error\0",
        _ => b"Unknown error\0",
    };
    env.mem
        .alloc_and_write_cstr(&msg[..msg.len() - 1])
        .cast_const()
}

/// `gethostent` iterates the hosts database. We don't model one, so signal
/// end-of-database (NULL) immediately, matching how `gethostbyname` reports an
/// unresolved host.
fn gethostent(_env: &mut Environment) -> MutPtr<hostent_guest> {
    log!("TODO: gethostent() => NULL");
    // TODO: set h_errno
    Ptr::null()
}

pub const FUNCTIONS: FunctionExports = &[
    export_c_func!(getaddrinfo(_, _, _, _)),
    export_c_func!(freeaddrinfo(_)),
    export_c_func!(gethostbyname(_)),
    export_c_func!(gethostent()),
    export_c_func!(gethostbyname2(_, _)),
    export_c_func!(gethostbyaddr(_, _, _)),
    export_c_func!(getservbyname(_, _)),
    export_c_func!(getservbyport(_, _)),
    export_c_func!(getnameinfo(_, _, _, _, _, _, _)),
    export_c_func!(__h_errno_location()),
    export_c_func!(gai_strerror(_)),
];

/// Non-lazy symbol export for `extern int h_errno;` as declared in
/// `<netdb.h>` (and historically by libresolv). Apps that bypass the
/// macro and reference the global directly (e.g. some POSIX C wrappers
/// emitted by older toolchains, sqlite's optional DNS helper) get the
/// stable cell allocated in `State::h_errno_cell`.
pub const CONSTANTS: ConstantExports = &[(
    "_h_errno",
    HostConstant::Custom(|env| -> ConstVoidPtr { h_errno_ptr(env).cast().cast_const() }),
)];
