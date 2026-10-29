//! Qt's QSettings stores, read the way Qt 6.12 reads them (`qsettings.cpp`,
//! `qsettings_mac.cpp`, and QVariant's conversions in `qmetatype.cpp`). Only
//! reading: the client never writes Qt's store.

use std::collections::BTreeMap;

/// A value as QSettings returns it, before a caller converts it.
#[derive(Clone, Debug, PartialEq)]
pub enum QtValue {
    /// `@Invalid()`, or a type the client does not read (`@Variant`, a map).
    Invalid,
    Bool(bool),
    Int(i64),
    Double(f64),
    Text(String),
    Bytes(Vec<u8>),
    List(Vec<QtValue>),
}

impl QtValue {
    /// `QVariant::toBool()`.
    pub fn to_bool(&self) -> bool {
        match self {
            QtValue::Bool(on) => *on,
            QtValue::Int(number) => *number != 0,
            QtValue::Double(number) => *number != 0.0,
            QtValue::Text(text) => text_to_bool(text),
            QtValue::Bytes(bytes) => text_to_bool(&String::from_utf8_lossy(bytes)),
            QtValue::Invalid | QtValue::List(_) => false,
        }
    }

    /// `QVariant::toDouble(&ok)`: a failed conversion gives 0.
    pub fn to_double(&self) -> (f64, bool) {
        match self {
            QtValue::Bool(on) => (f64::from(u8::from(*on)), true),
            QtValue::Int(number) => (*number as f64, true),
            QtValue::Double(number) => (*number, true),
            QtValue::Text(text) => parse_double(text),
            QtValue::Bytes(bytes) => parse_double(&String::from_utf8_lossy(bytes)),
            QtValue::Invalid | QtValue::List(_) => (0.0, false),
        }
    }

    /// `QVariant::toLongLong(&ok)`: a failed conversion gives 0, and a
    /// double rounds half away from zero.
    pub fn to_long(&self) -> (i64, bool) {
        match self {
            QtValue::Bool(on) => (i64::from(*on), true),
            QtValue::Int(number) => (*number, true),
            QtValue::Double(number) if number.is_nan() => (0, false),
            QtValue::Double(number) => (number.round() as i64, true),
            QtValue::Text(text) => parse_long(text),
            QtValue::Bytes(bytes) => parse_long(&String::from_utf8_lossy(bytes)),
            QtValue::Invalid | QtValue::List(_) => (0, false),
        }
    }

    /// `QVariant::toString()`: a list converts only with exactly one item.
    pub fn to_text(&self) -> String {
        match self {
            QtValue::Bool(on) => (if *on { "true" } else { "false" }).into(),
            QtValue::Int(number) => number.to_string(),
            QtValue::Double(number) => shortest(*number),
            QtValue::Text(text) => text.clone(),
            QtValue::Bytes(bytes) => String::from_utf8_lossy(bytes).into_owned(),
            QtValue::List(items) if items.len() == 1 => items[0].to_text(),
            QtValue::Invalid | QtValue::List(_) => String::new(),
        }
    }

    /// `QVariant::toStringList()`: only a string or a list converts.
    pub fn to_text_list(&self) -> Vec<String> {
        match self {
            QtValue::List(items) => items.iter().map(QtValue::to_text).collect(),
            QtValue::Text(text) => vec![text.clone()],
            _ => Vec::new(),
        }
    }
}

fn text_to_bool(text: &str) -> bool {
    let lower = text.to_lowercase();
    !(lower.is_empty() || lower == "0" || lower == "false")
}

fn parse_double(text: &str) -> (f64, bool) {
    match text.trim().parse::<f64>() {
        Ok(number) => (number, true),
        Err(_) => (0.0, false),
    }
}

fn parse_long(text: &str) -> (i64, bool) {
    match text.trim().parse::<i64>() {
        Ok(number) => (number, true),
        Err(_) => (0, false),
    }
}

/// `QString::number(d, 'g', QLocale::FloatingPointShortest)` for the values
/// settings hold.
fn shortest(number: f64) -> String {
    if number.is_finite() && number.fract() == 0.0 && number.abs() < 1e16 {
        format!("{}", number as i64)
    } else {
        format!("{number}")
    }
}

/// `QSettingsPrivate::stringToVariant`.
fn string_to_value(text: String) -> QtValue {
    if text.starts_with('@') {
        if text.ends_with(')') {
            if let Some(inner) = text.strip_prefix("@ByteArray(") {
                // toLatin1 of the characters.
                let inner = &inner[..inner.len() - 1];
                return QtValue::Bytes(inner.chars().map(|c| c as u32 as u8).collect());
            }
            if let Some(inner) = text.strip_prefix("@String(") {
                return QtValue::Text(inner[..inner.len() - 1].to_owned());
            }
            if text == "@Invalid()"
                || ["@Variant(", "@DateTime(", "@Rect(", "@Size(", "@Point("]
                    .iter()
                    .any(|prefix| text.starts_with(prefix))
            {
                return QtValue::Invalid;
            }
        }
        if let Some(rest) = text.strip_prefix("@@") {
            return QtValue::Text(format!("@{rest}"));
        }
    }
    QtValue::Text(text)
}

/// `QSettingsPrivate::stringListToVariantList`.
fn list_to_value(items: Vec<String>) -> QtValue {
    if items
        .iter()
        .any(|item| item.starts_with('@') && !item.starts_with("@@"))
    {
        return QtValue::List(items.into_iter().map(string_to_value).collect());
    }
    // Every `@` item here is an escaped `@@`.
    QtValue::List(
        items
            .into_iter()
            .map(|item| QtValue::Text(item.strip_prefix('@').map_or(item.clone(), str::to_owned)))
            .collect(),
    )
}

/// `QSettingsPrivate::iniUnescapedKey`.
fn unescape_key(key: &[u8]) -> String {
    let decoded: Vec<char> = String::from_utf8_lossy(key).chars().collect();
    let mut result = String::new();
    let mut i = 0;
    while i < decoded.len() {
        let ch = decoded[i];
        if ch == '\\' {
            result.push('/');
            i += 1;
            continue;
        }
        if ch != '%' || i == decoded.len() - 1 {
            result.push(ch);
            i += 1;
            continue;
        }
        let (first, digits) = if decoded[i + 1] == 'U' {
            (i + 2, 4)
        } else {
            (i + 1, 2)
        };
        if first + digits > decoded.len() {
            result.push('%');
            i += 1;
            continue;
        }
        let hex: String = decoded[first..first + digits].iter().collect();
        match u16::from_str_radix(&hex, 16)
            .ok()
            .and_then(|code| char::from_u32(u32::from(code)))
        {
            Some(code) => {
                result.push(code);
                i = first + digits;
            }
            None => {
                result.push('%');
                i += 1;
            }
        }
    }
    result
}

/// `QSettingsPrivate::iniUnescapedStringList`: the value, and whether it is
/// a list.
fn unescape_value(bytes: &[u8]) -> (String, Option<Vec<String>>) {
    let mut text: Vec<u16> = Vec::new();
    let mut list: Option<Vec<String>> = None;
    let mut in_quotes = false;
    let mut quoted = false;
    let mut i = 0;
    let push_str = |text: &mut Vec<u16>, part: &[u8]| {
        text.extend(String::from_utf8_lossy(part).encode_utf16());
    };
    let chop = |text: &mut Vec<u16>, limit: usize| {
        while text.len() > limit && matches!(text.last(), Some(&0x20) | Some(&0x09)) {
            text.pop();
        }
    };
    // StSkipSpaces
    let skip_spaces = |i: &mut usize| {
        while *i < bytes.len() && (bytes[*i] == b' ' || bytes[*i] == b'\t') {
            *i += 1;
        }
    };
    skip_spaces(&mut i);
    let mut chop_limit = text.len();
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => {
                i += 1;
                if i >= bytes.len() {
                    break;
                }
                let ch = bytes[i];
                i += 1;
                let simple = match ch {
                    b'a' => Some(0x07),
                    b'b' => Some(0x08),
                    b'f' => Some(0x0c),
                    b'n' => Some(0x0a),
                    b'r' => Some(0x0d),
                    b't' => Some(0x09),
                    b'v' => Some(0x0b),
                    b'"' | b'?' | b'\'' | b'\\' => Some(u16::from(ch)),
                    _ => None,
                };
                if let Some(code) = simple {
                    text.push(code);
                    chop_limit = text.len().min(chop_limit);
                    continue;
                }
                if ch == b'x' {
                    if i < bytes.len() && bytes[i].is_ascii_hexdigit() {
                        let mut value: u16 = 0;
                        while i < bytes.len() && bytes[i].is_ascii_hexdigit() {
                            value = value
                                .wrapping_shl(4)
                                .wrapping_add((bytes[i] as char).to_digit(16).unwrap_or(0) as u16);
                            i += 1;
                        }
                        text.push(value);
                    } else if i >= bytes.len() {
                        break;
                    }
                } else if (b'0'..=b'7').contains(&ch) {
                    let mut value = u16::from(ch - b'0');
                    while i < bytes.len() && (b'0'..=b'7').contains(&bytes[i]) {
                        value = value
                            .wrapping_shl(3)
                            .wrapping_add(u16::from(bytes[i] - b'0'));
                        i += 1;
                    }
                    text.push(value);
                } else if (ch == b'\n' || ch == b'\r') && i < bytes.len() {
                    let next = bytes[i];
                    if (next == b'\n' || next == b'\r') && next != ch {
                        i += 1;
                    }
                }
                chop_limit = text.len();
            }
            b'"' => {
                i += 1;
                quoted = true;
                in_quotes = !in_quotes;
                if !in_quotes {
                    skip_spaces(&mut i);
                    chop_limit = text.len();
                }
            }
            b',' if !in_quotes => {
                if !quoted {
                    chop(&mut text, chop_limit);
                }
                list.get_or_insert_with(Vec::new)
                    .push(String::from_utf16_lossy(&text));
                text.clear();
                quoted = false;
                i += 1;
                skip_spaces(&mut i);
                chop_limit = 0;
            }
            _ => {
                let mut j = i + 1;
                while j < bytes.len() && !matches!(bytes[j], b'\\' | b'"' | b',') {
                    j += 1;
                }
                push_str(&mut text, &bytes[i..j]);
                i = j;
            }
        }
    }
    if !quoted {
        chop(&mut text, chop_limit);
    }
    let text = String::from_utf16_lossy(&text);
    match list {
        Some(mut items) => {
            items.push(text.clone());
            (text, Some(items))
        }
        None => (text, None),
    }
}

/// One logical line of an INI file (`readIniLine`): its byte range and the
/// position of its first unquoted `=`.
fn ini_line(data: &[u8], pos: &mut usize) -> Option<(usize, usize, Option<usize>)> {
    let space = |ch: u8| matches!(ch, b'\t' | b'\n' | b'\r' | b' ');
    let special = |ch: u8| matches!(ch, b'\n' | b'\r' | b'"' | b';' | b'=' | b'\\');
    let mut start = *pos;
    while start < data.len() && space(data[start]) {
        start += 1;
    }
    let mut equals = None;
    let mut in_quotes = false;
    let mut i = start;
    'outer: while i < data.len() {
        let mut ch = data[i];
        while !special(ch) {
            i += 1;
            if i == data.len() {
                break 'outer;
            }
            ch = data[i];
        }
        i += 1;
        match ch {
            b'=' => {
                if !in_quotes && equals.is_none() {
                    equals = Some(i - 1);
                }
            }
            b'\n' | b'\r' => {
                if i == start + 1 {
                    start += 1;
                } else if !in_quotes {
                    i -= 1;
                    break 'outer;
                }
            }
            b'\\' => {
                if i < data.len() {
                    let first = data[i];
                    i += 1;
                    if i < data.len() {
                        let second = data[i];
                        if (first == b'\n' && second == b'\r')
                            || (first == b'\r' && second == b'\n')
                        {
                            i += 1;
                        }
                    }
                }
            }
            b'"' => in_quotes = !in_quotes,
            _ => {
                // ';'
                if i == start + 1 {
                    while i < data.len() && data[i] != b'\n' && data[i] != b'\r' {
                        i += 1;
                    }
                    while i < data.len() && space(data[i]) {
                        i += 1;
                    }
                    start = i;
                } else if !in_quotes {
                    i -= 1;
                    break 'outer;
                }
            }
        }
    }
    *pos = i;
    (i > start).then_some((start, i, equals))
}

/// Every key of a QSettings INI file (`readIniFile`, `readIniSection`), by
/// its Qt name (`workspace/theme`). A later line wins over an earlier one.
pub fn read_ini(data: &[u8]) -> BTreeMap<String, QtValue> {
    let data = data.strip_prefix(b"\xef\xbb\xbf").unwrap_or(data);
    let mut values = BTreeMap::new();
    let mut section = String::new();
    let mut pos = 0;
    while let Some((start, end, equals)) = ini_line(data, &mut pos) {
        let line = &data[start..end];
        if line.first() == Some(&b'[') {
            let close = line.iter().position(|&ch| ch == b']').unwrap_or(line.len());
            let name = line[1..close].trim_ascii();
            section = if name.eq_ignore_ascii_case(b"general") {
                String::new()
            } else if name.eq_ignore_ascii_case(b"%general") {
                format!("{}/", String::from_utf8_lossy(&name[1..]))
            } else {
                format!("{}/", unescape_key(name))
            };
            continue;
        }
        let Some(equals) = equals else {
            continue;
        };
        let key = data[start..equals].trim_ascii();
        let value = &data[equals + 1..end];
        let name = format!("{section}{}", unescape_key(key));
        let parsed = match unescape_value(value) {
            (_, Some(items)) => list_to_value(items),
            (text, None) => string_to_value(text),
        };
        values.insert(name, parsed);
    }
    values
}

/// `rotateSlashesDotsAndMiddots(key, Qtify)`: a macOS preference key back to
/// its Qt name (`workspace.theme` to `workspace/theme`).
pub fn qt_key(mac: &str) -> String {
    mac.chars()
        .map(|ch| match ch {
            '/' => '\u{b7}',
            '.' => '/',
            '\u{b7}' => '.',
            other => other,
        })
        .collect()
}

/// A macOS preference value as Qt reads it (`qtValue` in qsettings_mac.cpp).
pub fn plist_value(value: &plist::Value) -> QtValue {
    match value {
        plist::Value::String(text) => string_to_value(text.clone()),
        plist::Value::Boolean(on) => QtValue::Bool(*on),
        plist::Value::Integer(number) => number.as_signed().map_or(QtValue::Invalid, QtValue::Int),
        plist::Value::Real(number) => QtValue::Double(*number),
        plist::Value::Array(items) => QtValue::List(items.iter().map(plist_value).collect()),
        plist::Value::Data(bytes) => {
            if bytes.first() == Some(&b'@') {
                let text = String::from_utf8_lossy(bytes).into_owned();
                match string_to_value(text.clone()) {
                    QtValue::Text(same) if same == text => QtValue::Bytes(bytes.clone()),
                    decoded => decoded,
                }
            } else {
                QtValue::Bytes(bytes.clone())
            }
        }
        _ => QtValue::Invalid,
    }
}

/// Every key of Qt's macOS preference file, by its Qt name.
pub fn read_plist(data: &[u8]) -> Result<BTreeMap<String, QtValue>, String> {
    let value =
        plist::Value::from_reader(std::io::Cursor::new(data)).map_err(|error| error.to_string())?;
    let dictionary = value
        .into_dictionary()
        .ok_or_else(|| "not a dictionary".to_owned())?;
    Ok(dictionary
        .iter()
        .map(|(key, value)| (qt_key(key), plist_value(value)))
        .collect())
}
