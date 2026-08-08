//! Minimal PDH multi-instance counter reader (Windows only).
//!
//! One instance per counter path (`\GPU Engine(*)\Utilization Percentage`,
//! `\LogicalDisk(*)\Disk Read Bytes/sec`, ...).  `sample()` collects and
//! formats every instance; the first call after creation primes the rate
//! baseline and returns an empty Vec (same convention as the sysinfo
//! samplers: first reading is 0).

use windows::core::PCWSTR;
use windows::Win32::System::Performance::*;

pub struct PdhMultiCounter {
    query: PDH_HQUERY,
    counter: PDH_HCOUNTER,
    primed: bool,
}

// PDH handles are process-wide pointers; each PdhMultiCounter is only ever
// touched through its owning Mutex, so sending it across threads is fine.
unsafe impl Send for PdhMultiCounter {}

impl PdhMultiCounter {
    pub fn new(path: &str) -> Option<Self> {
        unsafe {
            let mut query = PDH_HQUERY::default();
            if PdhOpenQueryW(None, 0, &mut query) != 0 {
                return None;
            }
            let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
            let mut counter = PDH_HCOUNTER::default();
            // English counter path works regardless of the system UI language.
            if PdhAddEnglishCounterW(query, PCWSTR(wide.as_ptr()), 0, &mut counter) != 0 {
                let _ = PdhCloseQuery(query);
                return None;
            }
            let _ = PdhCollectQueryData(query); // baseline for rate counters
            Some(PdhMultiCounter {
                query,
                counter,
                primed: false,
            })
        }
    }

    /// (instance name, value) for every current instance.
    /// Empty on the priming call and on any PDH failure.
    pub fn sample(&mut self) -> Vec<(String, f64)> {
        unsafe {
            if PdhCollectQueryData(self.query) != 0 {
                return Vec::new();
            }
        }
        if !self.primed {
            self.primed = true;
            return Vec::new();
        }
        self.formatted_array()
            .into_iter()
            .filter(|(_, _, cstatus)| *cstatus == 0)
            .map(|(name, value, _)| (name, value))
            .collect()
    }

    /// 当前实例名列表（忽略各项数值状态）。
    /// 供「计数器里有哪些实例」的即时查询：实例名在基线采集后即就位，不受
    /// 速率计数器两阶段采样限制。会自行触发一次数据采集——对速率计数器而言
    /// 这只是把基线推进到此刻，不改变 sample() 的既有语义。
    pub fn instance_names(&mut self) -> Vec<String> {
        unsafe {
            if PdhCollectQueryData(self.query) != 0 {
                return Vec::new();
            }
        }
        self.formatted_array()
            .into_iter()
            .map(|(name, _, _)| name)
            .collect()
    }

    /// 取回当前格式化实例数组（名字、值、各项状态码）。
    /// 不采集——调用前须已 PdhCollectQueryData。
    fn formatted_array(&self) -> Vec<(String, f64, u32)> {
        let mut out = Vec::new();
        unsafe {
            let mut size: u32 = 0;
            let mut count: u32 = 0;
            let first = PdhGetFormattedCounterArrayW(
                self.counter,
                PDH_FMT_DOUBLE,
                &mut size,
                &mut count,
                None,
            );
            if (first != PDH_MORE_DATA && first != 0) || size == 0 {
                return out;
            }
            let mut buf = vec![0u8; size as usize];
            if PdhGetFormattedCounterArrayW(
                self.counter,
                PDH_FMT_DOUBLE,
                &mut size,
                &mut count,
                Some(buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W),
            ) != 0
            {
                return out;
            }
            let items = std::slice::from_raw_parts(
                buf.as_ptr() as *const PDH_FMT_COUNTERVALUE_ITEM_W,
                count as usize,
            );
            for item in items {
                let name = String::from_utf16_lossy(item.szName.as_wide());
                out.push((name, item.FmtValue.Anonymous.doubleValue, item.FmtValue.CStatus));
            }
        }
        out
    }
}

impl Drop for PdhMultiCounter {
    fn drop(&mut self) {
        unsafe {
            let _ = PdhCloseQuery(self.query);
        }
    }
}
