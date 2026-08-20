//! 极简 bencode 解析：从 .torrent 文件里取出 info 字典的编码字节，用于计算 infohash
//! （不依赖完整 bencode 解码库，够用即可）

/// 在顶层 dict 中取指定 key 的 value 字节切片
pub fn dict_get<'a>(data: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    if data.first() != Some(&b'd') {
        return None;
    }
    let mut i = 1usize;
    while i < data.len() {
        let (slen, sbytes) = parse_string(data, i)?;
        i = slen;
        let (vstart, vend) = value_bounds(data, i)?;
        if sbytes == key {
            return Some(&data[vstart..vend]);
        }
        i = vend;
    }
    None
}

/// 取顶层 dict 的字符串值（如 announce），返回解码后的内容
pub fn dict_get_str<'a>(data: &'a [u8], key: &[u8]) -> Option<&'a str> {
    if data.first() != Some(&b'd') {
        return None;
    }
    let mut i = 1usize;
    while i < data.len() {
        let (slen, sbytes) = parse_string(data, i)?;
        if sbytes == key {
            let (_, content) = parse_string(data, slen)?;
            return std::str::from_utf8(content).ok();
        }
        i = slen;
        let (_, ve) = value_bounds(data, i)?;
        i = ve;
    }
    None
}

/// 解析 [[字符串]] 嵌套列表（如 announce-list），i 指向外层 'l'
pub fn list_of_lists_strings(data: &[u8], i: usize) -> Option<Vec<String>> {
    if data.get(i) != Some(&b'l') {
        return None;
    }
    let mut j = i + 1;
    let mut out = Vec::new();
    while data.get(j) != Some(&b'e') {
        if data.get(j) == Some(&b'l') {
            let mut k = j + 1;
            while data.get(k) != Some(&b'e') {
                let (end, s) = parse_string(data, k)?;
                out.push(std::str::from_utf8(s).ok()?.to_string());
                k = end;
            }
            j = k + 1;
        } else {
            j = skip_value(data, j)?;
        }
    }
    Some(out)
}

/// 解析形如 "4:info" 的字符串，返回 (字符串结束位置, 内容字节)
fn parse_string(data: &[u8], i: usize) -> Option<(usize, &[u8])> {
    let colon = data[i..].iter().position(|&b| b == b':')?;
    let len_str = std::str::from_utf8(&data[i..i + colon]).ok()?;
    let len: usize = len_str.parse().ok()?;
    let content_start = i + colon + 1;
    let end = content_start.checked_add(len)?;
    if end > data.len() {
        return None;
    }
    Some((end, &data[content_start..end]))
}

/// 返回 value 的 (起始, 结束) 下标
fn value_bounds(data: &[u8], i: usize) -> Option<(usize, usize)> {
    let end = skip_value(data, i)?;
    Some((i, end))
}

/// 跳过 i 处的一个 bencode 值，返回结束下标
fn skip_value(data: &[u8], i: usize) -> Option<usize> {
    match data.get(i)? {
        b'i' => {
            let e = data[i..].iter().position(|&b| b == b'e')?;
            Some(i + e + 1)
        }
        b'l' => {
            let mut j = i + 1;
            while data.get(j)? != &b'e' {
                j = skip_value(data, j)?;
            }
            Some(j + 1)
        }
        b'd' => {
            let mut j = i + 1;
            while data.get(j)? != &b'e' {
                let (slen, _) = parse_string(data, j)?;
                j = skip_value(data, slen)?;
            }
            Some(j + 1)
        }
        _ => {
            let (end, _) = parse_string(data, i)?;
            Some(end)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn simple_torrent() -> Vec<u8> {
        // { "announce":"http://x", "info":{ "name":"a", "length":5 } }
        b"d8:announce8:http://x4:infod4:name1:a6:lengthi5eee".to_vec()
    }

    #[test]
    fn extract_info() {
        let data = simple_torrent();
        let info = dict_get(&data, b"info").unwrap();
        assert_eq!(info, b"d4:name1:a6:lengthi5ee");
    }

    #[test]
    fn missing_key() {
        let data = simple_torrent();
        assert!(dict_get(&data, b"nope").is_none());
    }

    #[test]
    fn known_infohash() {
        // 来自真实样例结构，验证 SHA1 计算路径不 panic 且长度 20
        let data = simple_torrent();
        let info = dict_get(&data, b"info").unwrap();
        use sha1::Digest;
        let h = sha1::Sha1::digest(info);
        assert_eq!(h.len(), 20);
    }
}
