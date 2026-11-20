//! [INPUT]: 依赖 Windows IP Helper 的原生适配器缓冲区。
//! [OUTPUT]: 向地址诊断提供全部单播接口，不筛选 MAC 或链路速率。
//! [POS]: Collector 的 Windows 地址来源；严格计数仍由 sysinfo 提供。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use std::{
    collections::HashSet,
    mem::{align_of, size_of},
    net::IpAddr,
};
use windows_sys::Win32::{
    Foundation::{ERROR_BUFFER_OVERFLOW, ERROR_NO_DATA, NO_ERROR},
    NetworkManagement::IpHelper::{
        GAA_FLAG_INCLUDE_ALL_INTERFACES, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER,
        GAA_FLAG_SKIP_MULTICAST, IP_ADAPTER_ADDRESSES_LH, IP_ADAPTER_UNICAST_ADDRESS_LH,
    },
    Networking::WinSock::{
        AF_INET, AF_INET6, AF_UNSPEC, SOCKADDR_IN, SOCKADDR_IN6, SOCKET_ADDRESS,
    },
};

const INITIAL_BYTES: usize = 15 * 1024;
const MAX_BUFFER_BYTES: usize = 1024 * 1024;
const _: () = assert!(align_of::<IP_ADAPTER_ADDRESSES_LH>() <= align_of::<u64>());

#[cfg(windows)]
pub(super) fn collect() -> Option<Vec<(String, Vec<IpAddr>)>> {
    collect_with(|family, flags, pointer, length| {
        // 缓冲区按适配器对齐，长度对应本次分配；API 不保留这些指针。
        unsafe {
            windows_sys::Win32::NetworkManagement::IpHelper::GetAdaptersAddresses(
                family,
                flags,
                std::ptr::null(),
                pointer,
                length,
            )
        }
    })
}

fn collect_with(
    mut query: impl FnMut(u32, u32, *mut IP_ADAPTER_ADDRESSES_LH, *mut u32) -> u32,
) -> Option<Vec<(String, Vec<IpAddr>)>> {
    let mut capacity = INITIAL_BYTES;
    for _ in 0..3 {
        if capacity > MAX_BUFFER_BYTES {
            return None;
        }
        let mut buffer = Buffer {
            words: vec![0u64; capacity.div_ceil(size_of::<u64>())],
            used: capacity,
        };
        let mut length = capacity as u32;
        let status = query(
            AF_UNSPEC as u32,
            GAA_FLAG_INCLUDE_ALL_INTERFACES
                | GAA_FLAG_SKIP_ANYCAST
                | GAA_FLAG_SKIP_MULTICAST
                | GAA_FLAG_SKIP_DNS_SERVER,
            buffer.words.as_mut_ptr().cast(),
            &mut length,
        );
        match status {
            NO_ERROR if length as usize <= capacity => {
                buffer.used = length as usize;
                return buffer.interfaces();
            }
            ERROR_NO_DATA => return Some(vec![]),
            ERROR_BUFFER_OVERFLOW if length as usize > capacity => capacity = length as usize,
            _ => return None,
        }
    }
    None
}

struct Buffer {
    words: Vec<u64>,
    used: usize,
}
impl Buffer {
    fn range(&self, pointer: *const u8, bytes: usize) -> Option<usize> {
        let offset = (pointer as usize).checked_sub(self.words.as_ptr() as usize)?;
        (offset.checked_add(bytes)? <= self.used).then_some(offset)
    }

    fn read<T: Copy>(&self, pointer: *const T) -> Option<T> {
        let offset = self.range(pointer.cast(), size_of::<T>())?;
        // 只从当前分配的已初始化字节读取。原生链指针不能越界或要求对齐。
        Some(unsafe {
            self.words
                .as_ptr()
                .cast::<u8>()
                .add(offset)
                .cast::<T>()
                .read_unaligned()
        })
    }

    fn name(&self, pointer: *const u16) -> Option<String> {
        let offset = self.range(pointer.cast(), size_of::<u16>())?;
        let mut units = Vec::new();
        for index in 0..(self.used - offset) / size_of::<u16>() {
            // offset 与循环上限限定在当前分配内，read 同时检查每个单元。
            let pointer = unsafe {
                self.words
                    .as_ptr()
                    .cast::<u8>()
                    .add(offset + index * 2)
                    .cast::<u16>()
            };
            let unit = self.read(pointer)?;
            if unit == 0 {
                return String::from_utf16(&units).ok();
            }
            units.push(unit);
        }
        None
    }

    fn address(&self, socket: SOCKET_ADDRESS) -> Option<Option<IpAddr>> {
        let length = usize::try_from(socket.iSockaddrLength).ok()?;
        if length < size_of::<u16>() {
            return None;
        }
        self.range(socket.lpSockaddr.cast(), length)?;
        let family = self.read(socket.lpSockaddr.cast::<u16>())?;
        Some(match family {
            AF_INET if length >= size_of::<SOCKADDR_IN>() => {
                let address = self.read(socket.lpSockaddr.cast::<SOCKADDR_IN>())?;
                // S_addr 以网络字节序存放；读取内存字节以免主机字节序颠倒。
                Some(IpAddr::from(
                    unsafe { address.sin_addr.S_un.S_addr }.to_ne_bytes(),
                ))
            }
            AF_INET6 if length >= size_of::<SOCKADDR_IN6>() => {
                let address = self.read(socket.lpSockaddr.cast::<SOCKADDR_IN6>())?;
                Some(IpAddr::from(unsafe { address.sin6_addr.u.Byte }))
            }
            AF_INET | AF_INET6 => return None,
            _ => None,
        })
    }

    fn interfaces(&self) -> Option<Vec<(String, Vec<IpAddr>)>> {
        let mut pointer = self.words.as_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
        let mut seen = HashSet::new();
        let mut interfaces = Vec::new();
        while !pointer.is_null() {
            if !seen.insert(pointer as usize) {
                return None;
            }
            let adapter = self.read(pointer)?;
            if unsafe { adapter.Anonymous1.Anonymous.Length }
                < size_of::<IP_ADAPTER_ADDRESSES_LH>() as u32
            {
                return None;
            }
            if let Some(name) = self
                .name(adapter.FriendlyName)
                .filter(|name| !name.is_empty())
            {
                let mut addresses = Vec::new();
                let mut unicast = adapter.FirstUnicastAddress;
                let mut seen = HashSet::new();
                while !unicast.is_null() {
                    if !seen.insert(unicast as usize) {
                        return None;
                    }
                    let entry = self.read(unicast)?;
                    if unsafe { entry.Anonymous.Anonymous.Length }
                        < size_of::<IP_ADAPTER_UNICAST_ADDRESS_LH>() as u32
                    {
                        return None;
                    }
                    if let Some(address) = self.address(entry.Address)? {
                        addresses.push(address);
                    }
                    unicast = entry.Next;
                }
                interfaces.push((name, addresses));
            }
            pointer = adapter.Next;
        }
        Some(interfaces)
    }
}

#[cfg(test)]
#[path = "windows_network_tests.rs"]
mod tests;
