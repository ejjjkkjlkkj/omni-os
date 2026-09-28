//! Shared conservative FR/EN normalization. Ambiguous formats remain literal.
//!
//! Handles what a screen reader meets constantly and engines misread: dates,
//! times, decimals, titles, percentages, amounts, French ordinals, Windows paths,
//! URLs, e-mail addresses, IPv4 addresses and file extensions.
pub fn normalize(text: &str, french: bool) -> String {
    let tokens: Vec<&str> = text.split_whitespace().collect();
    let mut out = Vec::with_capacity(tokens.len());
    for (i, token) in tokens.iter().enumerate() {
        let after_number = i > 0 && tokens[i - 1].bytes().any(|c| c.is_ascii_digit());
        out.push(match (*token, after_number) {
            ("%", true) => w(french, "pour cent", "percent").into(),
            ("€", true) => "euros".into(),
            ("$", true) => "dollars".into(),
            _ => normalize_token(token, french),
        });
    }
    out.join(" ")
}

fn w(fr: bool, french: &'static str, english: &'static str) -> &'static str {
    if fr {
        french
    } else {
        english
    }
}

const EXTENSIONS: &[&str] = &[
    "txt", "doc", "docx", "pdf", "xls", "xlsx", "ppt", "pptx", "exe", "dll", "zip", "png", "jpg",
    "jpeg", "gif", "wav", "mp3", "mp4", "html", "htm", "json", "xml", "csv", "md", "rs", "py",
    "ps1", "bat", "efi", "iso", "msi", "log", "ini", "cfg", "sys", "inf", "lnk", "url",
];

fn is_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit())
}

/// "a.b-c" -> "a point b-c": dots spoken, other characters kept for the engine.
fn spell_dots(s: &str, fr: bool) -> String {
    s.split('.')
        .filter(|x| !x.is_empty())
        .collect::<Vec<_>>()
        .join(w(fr, " point ", " dot "))
}

fn decimal(a: &str, b: &str, fr: bool) -> String {
    let digits = if fr {
        [
            "zéro", "un", "deux", "trois", "quatre", "cinq", "six", "sept", "huit", "neuf",
        ]
    } else {
        [
            "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine",
        ]
    };
    let fraction = b
        .bytes()
        .map(|c| digits[(c - b'0') as usize])
        .collect::<Vec<_>>()
        .join(" ");
    format!("{a} {} {fraction}", w(fr, "virgule", "point"))
}

/// A number as written in the language ("12", "12,5" in French, "12.5" in English).
fn number(s: &str, fr: bool) -> Option<String> {
    if is_digits(s) {
        return Some(s.into());
    }
    let (a, b) = s.split_once(if fr { ',' } else { '.' })?;
    (is_digits(a) && is_digits(b)).then(|| decimal(a, b, fr))
}

fn money(amount: &str, unit: &str, fr: bool) -> Option<String> {
    if is_digits(amount) {
        return Some(format!("{amount} {unit}"));
    }
    let (a, b) = amount.split_once(if fr { ',' } else { '.' })?;
    if !(is_digits(a) && is_digits(b) && b.len() == 2) {
        return None;
    }
    // "12,50 €" -> "12 euros 50"; "12,00 €" -> "12 euros"; "12,05 €" -> "12 euros 5"
    let cents = b.trim_start_matches('0');
    Some(if cents.is_empty() {
        format!("{a} {unit}")
    } else {
        format!("{a} {unit} {cents}")
    })
}

fn normalize_token(token: &str, fr: bool) -> String {
    let core = token.trim_end_matches(['.', ',', ';', '!', '?', ')', '»', '"']);
    let core = if core.is_empty() { token } else { core };
    let tail = &token[core.len()..];
    let abbr = match (fr, token) {
        (true, "M.") => Some("monsieur"),
        (true, "Mme") | (true, "Mme.") => Some("madame"),
        (true, "Dr.") => Some("docteur"),
        (false, "Dr.") => Some("doctor"),
        (false, "Mr.") => Some("mister"),
        (false, "Mrs.") => Some("missus"),
        _ => None,
    };
    if let Some(a) = abbr {
        return a.into();
    }

    // Windows paths: C:\Users\adm\rapport.docx, \\server\share
    if (core.len() > 2
        && core.as_bytes()[1] == b':'
        && core.as_bytes()[0].is_ascii_alphabetic()
        && core[2..].starts_with('\\'))
        || core.starts_with("\\\\")
    {
        let mut parts = Vec::new();
        for (i, part) in core.split('\\').filter(|p| !p.is_empty()).enumerate() {
            parts.push(if i == 0 && part.ends_with(':') {
                format!(
                    "{} {}",
                    &part[..part.len() - 1],
                    w(fr, "deux-points", "colon")
                )
            } else {
                file_name(part, fr)
            });
        }
        return parts.join(", ") + tail;
    }
    // E-mail before URL: user.name@example.fr
    if let Some((user, host)) = core.split_once('@') {
        if !user.is_empty() && host.contains('.') && !host.contains('@') && !host.contains('/') {
            return format!(
                "{} {} {}{tail}",
                spell_dots(user, fr),
                w(fr, "arobase", "at"),
                spell_dots(host, fr)
            );
        }
    }
    // URLs: scheme dropped, dots and slashes spoken.
    let url = core
        .strip_prefix("https://")
        .or_else(|| core.strip_prefix("http://"))
        .or_else(|| core.starts_with("www.").then_some(core));
    if let Some(rest) = url {
        let path = rest
            .trim_end_matches('/')
            .split('/')
            .map(|p| spell_dots(p, fr))
            .collect::<Vec<_>>();
        return path.join(" slash ") + tail;
    }
    // IPv4: 192.168.1.10
    let octets: Vec<&str> = core.split('.').collect();
    if octets.len() == 4
        && octets.iter().all(|o| {
            is_digits(o) && o.len() <= 3 && o.parse::<u32>().map(|v| v < 256).unwrap_or(false)
        })
    {
        return octets.join(w(fr, " point ", " dot ")) + tail;
    }
    // Percentages: 12%, 12,5%
    if let Some(n) = core.strip_suffix('%').and_then(|n| number(n, fr)) {
        return format!("{n} {}{tail}", w(fr, "pour cent", "percent"));
    }
    // Amounts: 12€, 12,50€, €12, $12.50
    if let Some(m) = core
        .strip_suffix('€')
        .or_else(|| core.strip_prefix('€'))
        .and_then(|a| money(a, "euros", fr))
    {
        return m + tail;
    }
    if let Some(m) = core
        .strip_prefix('$')
        .or_else(|| core.strip_suffix('$'))
        .and_then(|a| money(a, "dollars", fr))
    {
        return m + tail;
    }
    // French ordinals: 1er, 1re, 2e, 3ème ... 10e
    if fr {
        let digits_end = core.bytes().take_while(|c| c.is_ascii_digit()).count();
        if digits_end > 0 {
            let (n, suffix) = core.split_at(digits_end);
            let n: u32 = n.parse().unwrap_or(0);
            let word = match (n, suffix) {
                (1, "er") => Some("premier".to_string()),
                (1, "re" | "ère") => Some("première".to_string()),
                (2..=10, "e" | "ème" | "eme" | "è") => Some(
                    [
                        "deuxième",
                        "troisième",
                        "quatrième",
                        "cinquième",
                        "sixième",
                        "septième",
                        "huitième",
                        "neuvième",
                        "dixième",
                    ][n as usize - 2]
                        .to_string(),
                ),
                _ => None,
            };
            if let Some(word) = word {
                return word + tail;
            }
        }
    }
    let time = core
        .split(if core.contains(':') { ':' } else { 'h' })
        .collect::<Vec<_>>();
    if time.len() == 2 {
        if let (Ok(h), Ok(m)) = (time[0].parse::<u32>(), time[1].parse::<u32>()) {
            if h < 24 && m < 60 {
                return if fr {
                    format!("{h} heures {m} minutes{tail}")
                } else {
                    format!("{h} hours {m} minutes{tail}")
                };
            }
        }
    }
    let date = core
        .split(if core.contains('/') { '/' } else { '-' })
        .collect::<Vec<_>>();
    if date.len() == 3 {
        let nums = date
            .iter()
            .map(|x| x.parse::<u32>())
            .collect::<Result<Vec<_>, _>>();
        if let Ok(n) = nums {
            let (y, m, d) = if date[0].len() == 4 {
                (n[0], n[1], n[2])
            } else if fr && date[2].len() == 4 {
                (n[2], n[1], n[0])
            } else {
                (0, 0, 0)
            };
            let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
            let days = match m {
                2 => {
                    if leap {
                        29
                    } else {
                        28
                    }
                }
                4 | 6 | 9 | 11 => 30,
                1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
                _ => 0,
            };
            if y > 0 && y <= 9999 && d > 0 && d <= days {
                let months = if fr {
                    [
                        "janvier",
                        "février",
                        "mars",
                        "avril",
                        "mai",
                        "juin",
                        "juillet",
                        "août",
                        "septembre",
                        "octobre",
                        "novembre",
                        "décembre",
                    ]
                } else {
                    [
                        "January",
                        "February",
                        "March",
                        "April",
                        "May",
                        "June",
                        "July",
                        "August",
                        "September",
                        "October",
                        "November",
                        "December",
                    ]
                };
                let day = if fr && d == 1 {
                    "premier".into()
                } else {
                    d.to_string()
                };
                return format!("{day} {} {y}{tail}", months[m as usize - 1]);
            }
        }
    }
    let separator = if fr { ',' } else { '.' };
    if let Some((a, b)) = core.split_once(separator) {
        if is_digits(a) && is_digits(b) {
            return decimal(a, b, fr) + tail;
        }
    }
    // File names and bare extensions: rapport.docx, .docx
    let file = file_name(core, fr);
    if file != core {
        return file + tail;
    }
    token.into()
}

/// "rapport.docx" -> "rapport point docx" when the extension is a known file type.
fn file_name(name: &str, fr: bool) -> String {
    if let Some((stem, ext)) = name.rsplit_once('.') {
        if EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()) && !stem.ends_with('.') {
            let dot = w(fr, "point", "dot");
            return if stem.is_empty() {
                format!("{dot} {ext}")
            } else {
                format!("{stem} {dot} {ext}")
            };
        }
    }
    name.into()
}
