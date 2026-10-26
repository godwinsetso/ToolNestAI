//! Text normalisation: turns written text into what a careful reader would
//! say. The output is only ever sent to the speech engine; the document text
//! shown to the user is never changed.
//!
//! Conventions follow British/Ghanaian English: day-first dates, "and" in
//! numbers ("one hundred and five"), cedis and pesewas.

use crate::model::PronunciationRule;
use once_cell::sync::Lazy;
use regex::{Captures, Regex};

#[derive(Debug, Clone)]
pub struct NormalizeOptions {
    /// Spell acronyms letter by letter (AML -> "A M L") when they have no vowel or are short.
    pub spell_acronyms: bool,
    /// Treat 01/02/2026 as 1 February (true) or January 2 (false).
    pub day_first_dates: bool,
    /// Read every number digit by digit (spelling mode for serials and IDs).
    pub spell_mode: bool,
}

impl Default for NormalizeOptions {
    fn default() -> Self {
        Self { spell_acronyms: true, day_first_dates: true, spell_mode: false }
    }
}

const ONES: [&str; 20] = [
    "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven",
    "twelve", "thirteen", "fourteen", "fifteen", "sixteen", "seventeen", "eighteen", "nineteen",
];
const TENS: [&str; 10] = ["", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety"];
const MONTHS: [&str; 12] = [
    "January", "February", "March", "April", "May", "June", "July", "August", "September", "October",
    "November", "December",
];

fn under_thousand(n: u64, out: &mut Vec<String>) {
    let h = n / 100;
    let r = n % 100;
    if h > 0 {
        out.push(format!("{} hundred", ONES[h as usize]));
        if r > 0 {
            out.push("and".into());
        }
    }
    if r > 0 {
        if r < 20 {
            out.push(ONES[r as usize].into());
        } else if r % 10 == 0 {
            out.push(TENS[(r / 10) as usize].into());
        } else {
            out.push(format!("{}-{}", TENS[(r / 10) as usize], ONES[(r % 10) as usize]));
        }
    }
}

/// 1205 -> "one thousand, two hundred and five"
pub fn number_words(n: u64) -> String {
    if n == 0 {
        return "zero".into();
    }
    const SCALES: [(u64, &str); 4] = [
        (1_000_000_000_000, "trillion"),
        (1_000_000_000, "billion"),
        (1_000_000, "million"),
        (1_000, "thousand"),
    ];
    let mut parts: Vec<String> = Vec::new();
    let mut rest = n;
    for (value, name) in SCALES {
        if rest >= value {
            let mut w = Vec::new();
            under_thousand(rest / value, &mut w);
            parts.push(format!("{} {}", w.join(" "), name));
            rest %= value;
        }
    }
    if rest > 0 {
        let mut w = Vec::new();
        under_thousand(rest, &mut w);
        // "one thousand and five", but "one thousand, two hundred and five"
        if !parts.is_empty() && rest < 100 {
            parts.push(format!("and {}", w.join(" ")));
        } else {
            parts.push(w.join(" "));
        }
    }
    parts.join(", ").replace(", and", " and")
}

fn ordinal_words(n: u64) -> String {
    let w = number_words(n);
    let irregular = [
        ("one", "first"), ("two", "second"), ("three", "third"), ("five", "fifth"), ("eight", "eighth"),
        ("nine", "ninth"), ("twelve", "twelfth"),
    ];
    for (a, b) in irregular {
        if w.ends_with(a) {
            return format!("{}{}", &w[..w.len() - a.len()], b);
        }
    }
    if let Some(stem) = w.strip_suffix('y') {
        return format!("{stem}ieth");
    }
    format!("{w}th")
}

/// 1998 -> "nineteen ninety-eight"; 2005 -> "two thousand and five"; 2026 -> "twenty twenty-six"
fn year_words(y: u64) -> String {
    if (2000..2010).contains(&y) {
        return number_words(y);
    }
    let hi = y / 100;
    let lo = y % 100;
    let hi_w = number_words(hi);
    match lo {
        0 => format!("{hi_w} hundred"),
        1..=9 => format!("{hi_w} oh {}", ONES[lo as usize]),
        _ => format!("{hi_w} {}", number_words(lo)),
    }
}

fn digits_words(s: &str) -> String {
    s.chars()
        .filter_map(|c| c.to_digit(10).map(|d| ONES[d as usize].to_string()))
        .collect::<Vec<_>>()
        .join(" ")
}

/// "1,234.56" -> "one thousand, two hundred and thirty-four point five six"
fn decimal_words(s: &str) -> String {
    let clean: String = s.chars().filter(|c| *c != ',' && *c != ' ').collect();
    let (int, frac) = match clean.split_once('.') {
        Some((a, b)) => (a, Some(b)),
        None => (clean.as_str(), None),
    };
    let int_w = if int.is_empty() {
        "zero".to_string()
    } else if int.len() > 15 {
        digits_words(int)
    } else {
        int.parse::<u64>().map(number_words).unwrap_or_else(|_| digits_words(int))
    };
    match frac {
        Some(f) if !f.is_empty() => format!("{int_w} point {}", digits_words(f)),
        _ => int_w,
    }
}

fn scale_word(s: &str) -> Option<&'static str> {
    Some(match s.to_ascii_lowercase().as_str() {
        "k" | "thousand" => "thousand",
        "m" | "mn" | "million" => "million",
        "b" | "bn" | "billion" => "billion",
        "t" | "tn" | "trillion" => "trillion",
        _ => return None,
    })
}

fn currency_names(sym: &str) -> Option<(&'static str, &'static str, &'static str, &'static str)> {
    // (singular, plural, minor singular, minor plural)
    Some(match sym {
        "GH₵" | "GHS" | "GH¢" | "₵" | "GHC" | "¢" => ("cedi", "cedis", "pesewa", "pesewas"),
        "$" | "US$" | "USD" => ("dollar", "dollars", "cent", "cents"),
        "£" | "GBP" => ("pound", "pounds", "penny", "pence"),
        "€" | "EUR" => ("euro", "euros", "cent", "cents"),
        "₦" | "NGN" => ("naira", "naira", "kobo", "kobo"),
        "¥" | "JPY" => ("yen", "yen", "sen", "sen"),
        "CFA" | "XOF" => ("CFA franc", "CFA francs", "centime", "centimes"),
        _ => return None,
    })
}

fn money(sym: &str, amount: &str, scale: Option<&str>) -> String {
    let Some((one, many, minor1, minor_n)) = currency_names(sym) else {
        return format!("{sym} {amount}");
    };
    let plain: String = amount.chars().filter(|c| *c != ',').collect();
    if let Some(sc) = scale.and_then(scale_word) {
        return format!("{} {sc} {many}", decimal_words(&plain));
    }
    let (int, frac) = match plain.split_once('.') {
        Some((a, b)) => (a.to_string(), Some(b.to_string())),
        None => (plain.clone(), None),
    };
    let n: u64 = int.parse().unwrap_or(0);
    let mut s = format!("{} {}", number_words(n), if n == 1 { one } else { many });
    if let Some(f) = frac {
        let cents: u64 = format!("{:0<2}", &f[..f.len().min(2)]).parse().unwrap_or(0);
        if cents > 0 {
            s.push_str(&format!(" and {} {}", number_words(cents), if cents == 1 { minor1 } else { minor_n }));
        }
    }
    s
}

macro_rules! re {
    ($name:ident, $pat:expr) => {
        static $name: Lazy<Regex> = Lazy::new(|| Regex::new($pat).unwrap());
    };
}

re!(URL, r"\bhttps?://(?:www\.)?([A-Za-z0-9.-]+\.[A-Za-z]{2,})(?:/[^\s)]*)?");
re!(WWW, r"\bwww\.([A-Za-z0-9.-]+\.[A-Za-z]{2,})(?:/[^\s)]*)?");
re!(EMAIL, r"\b([A-Za-z0-9._%+-]+)@([A-Za-z0-9.-]+\.[A-Za-z]{2,})\b");
re!(
    MONEY_PRE,
    r"(GH₵|GH¢|GHS|GHC|US\$|USD|GBP|EUR|NGN|₵|\$|£|€|₦)\s?(\d[\d,]*(?:\.\d+)?)(?:\s?(k|m|mn|bn|b|tn|million|billion|thousand|trillion)\b)?"
);
re!(PERCENT, r"(\d[\d,]*(?:\.\d+)?)\s?%");
re!(ISO_DATE, r"\b(\d{4})-(\d{2})-(\d{2})\b");
re!(SLASH_DATE, r"\b(\d{1,2})[/.](\d{1,2})[/.](\d{4}|\d{2})\b");
re!(ORDINAL, r"\b(\d+)(st|nd|rd|th)\b");
re!(YEAR_RANGE, r"\b(1[1-9]\d{2}|20\d{2})\s?[–-]\s?(1[1-9]\d{2}|20\d{2}|\d{2})\b");
re!(NUM_RANGE, r"\b(\d+)\s?[–]\s?(\d+)\b");
re!(TIME, r"\b(\d{1,2}):(\d{2})\s?(am|pm|a\.m\.|p\.m\.)?");
re!(ALNUM_ID, r"\b(?:[A-Z]+\d[A-Z\d-]*|\d+[A-Z][A-Z\d-]*)\b");
re!(ACRONYM, r"\b[A-Z]{2,6}s?\b");
re!(NUMBER, r"\b\d{1,3}(?:,\d{3})+(?:\.\d+)?\b|\b\d+(?:\.\d+)?\b");
re!(UNIT, r"(\d)\s?(km|kg|cm|mm|mg|ml|kWh|MW|GB|MB|TB|°C|°F|m²|km²)\b");

/// Abbreviations expanded when they appear as whole tokens.
const ABBREVIATIONS: &[(&str, &str)] = &[
    ("e.g.", "for example"),
    ("i.e.", "that is"),
    ("etc.", "et cetera"),
    ("vs.", "versus"),
    ("approx.", "approximately"),
    ("Dr.", "Doctor"),
    ("Mr.", "Mister"),
    ("Mrs.", "Missus"),
    ("Prof.", "Professor"),
    ("Rev.", "Reverend"),
    ("Hon.", "Honourable"),
    ("Ltd.", "Limited"),
    ("Ltd", "Limited"),
    ("Co.", "Company"),
    ("Inc.", "Incorporated"),
    ("Dept.", "Department"),
    ("Fig.", "Figure"),
    ("No.", "Number"),
    ("Nos.", "Numbers"),
    ("Sec.", "Section"),
    ("Jan.", "January"),
    ("Feb.", "February"),
    ("Aug.", "August"),
    ("Sept.", "September"),
    ("Oct.", "October"),
    ("Nov.", "November"),
    ("Dec.", "December"),
];

/// Acronyms that are pronounced as words rather than spelled.
const SPOKEN_ACRONYMS: &[&str] = &[
    "NATO", "UNESCO", "UNICEF", "ECOWAS", "COVID", "AIDS", "NASA", "OPEC", "FIFA", "PIN", "RAM",
    "LAN", "WAN", "SIM", "GIF", "JPEG", "ASAP", "CEDAW", "SWIFT", "INTERPOL", "EOCO", "NADMO", "COCOBOD", "SSNIT",
];

const UNITS: &[(&str, &str)] = &[
    ("km", "kilometres"), ("kg", "kilograms"), ("cm", "centimetres"), ("mm", "millimetres"),
    ("mg", "milligrams"), ("ml", "millilitres"), ("kWh", "kilowatt hours"), ("MW", "megawatts"),
    ("GB", "gigabytes"), ("MB", "megabytes"), ("TB", "terabytes"), ("°C", "degrees Celsius"),
    ("°F", "degrees Fahrenheit"), ("m²", "square metres"), ("km²", "square kilometres"),
];

/// Applies the user's rules, replacing each match with a private-use
/// placeholder so later normalisation steps cannot alter the user's wording.
fn apply_rules(text: &str, rules: &[PronunciationRule], slots: &mut Vec<String>) -> String {
    let mut s = text.to_string();
    for r in rules {
        let pat = format!(r"(?{}){}{}{}",
            if r.case_sensitive { "" } else { "i" },
            if r.term.chars().next().map(|c| c.is_alphanumeric()).unwrap_or(false) { r"\b" } else { "" },
            regex::escape(&r.term),
            if r.term.chars().last().map(|c| c.is_alphanumeric()).unwrap_or(false) { r"\b" } else { "" });
        if let Ok(re) = Regex::new(&pat) {
            if re.is_match(&s) && slots.len() < 6000 {
                let marker = char::from_u32(0xE000 + slots.len() as u32).unwrap().to_string();
                slots.push(r.replacement.clone());
                s = re.replace_all(&s, regex::NoExpand(&marker)).into_owned();
            }
        }
    }
    s
}

fn restore_slots(s: &str, slots: &[String]) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let code = c as u32;
        if (0xE000..0xE000 + slots.len() as u32).contains(&code) {
            out.push_str(&slots[(code - 0xE000) as usize]);
        } else {
            out.push(c);
        }
    }
    out
}

/// Whether an all-capitals word is better spelled out than pronounced.
fn should_spell(core: &str) -> bool {
    let is_vowel = |c: char| "AEIOUY".contains(c);
    if core.len() <= 3 || !core.chars().any(is_vowel) {
        return true;
    }
    let chars: Vec<char> = core.chars().collect();
    let ends_in_cluster = chars.len() >= 2 && !is_vowel(chars[chars.len() - 1]) && !is_vowel(chars[chars.len() - 2]);
    let mut run = 0;
    let mut max_run = 0;
    for &c in &chars {
        run = if is_vowel(c) { 0 } else { run + 1 };
        max_run = max_run.max(run);
    }
    let starts_in_cluster = !is_vowel(chars[0]) && !is_vowel(chars[1]);
    ends_in_cluster || starts_in_cluster || max_run >= 3
}

fn spell(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .map(|c| match c.to_digit(10) {
            Some(d) => ONES[d as usize].to_string(),
            None => c.to_ascii_uppercase().to_string(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn month(m: u32) -> Option<&'static str> {
    if (1..=12).contains(&m) { Some(MONTHS[(m - 1) as usize]) } else { None }
}

fn date_words(d: u32, m: u32, y: u64) -> Option<String> {
    let name = month(m)?;
    if !(1..=31).contains(&d) {
        return None;
    }
    Some(format!("the {} of {} {}", ordinal_words(d as u64), name, year_words(y)))
}

/// Normalise text for speech.
pub fn normalize(text: &str, rules: &[PronunciationRule], opts: &NormalizeOptions) -> String {
    // 1. The user's own pronunciations win over everything else.
    let mut slots = Vec::new();
    let mut s = apply_rules(text, rules, &mut slots);

    // 2. Links and addresses.
    s = EMAIL
        .replace_all(&s, |c: &Captures| format!("{} at {}", &c[1], c[2].replace('.', " dot ")))
        .into_owned();
    s = URL.replace_all(&s, |c: &Captures| c[1].replace('.', " dot ")).into_owned();
    s = WWW.replace_all(&s, |c: &Captures| c[1].replace('.', " dot ")).into_owned();

    // 3. Abbreviations (token-wise so "Dr." inside words is untouched).
    for (abbr, full) in ABBREVIATIONS {
        let pat = format!(r"(^|[\s(]){}", regex::escape(abbr));
        let re = Regex::new(&pat).unwrap();
        s = re.replace_all(&s, |c: &Captures| format!("{}{}", &c[1], full)).into_owned();
    }

    // 4. Money, percentages, units, dates, times, ranges, ordinals.
    s = MONEY_PRE
        .replace_all(&s, |c: &Captures| money(&c[1], &c[2], c.get(3).map(|m| m.as_str())))
        .into_owned();
    s = PERCENT.replace_all(&s, |c: &Captures| format!("{} percent", decimal_words(&c[1]))).into_owned();
    s = UNIT
        .replace_all(&s, |c: &Captures| {
            let unit = UNITS.iter().find(|(u, _)| *u == &c[2]).map(|(_, w)| *w).unwrap_or(&c[2]);
            format!("{} {}", &c[1], unit)
        })
        .into_owned();
    s = ISO_DATE
        .replace_all(&s, |c: &Captures| {
            date_words(c[3].parse().unwrap_or(0), c[2].parse().unwrap_or(0), c[1].parse().unwrap_or(0))
                .unwrap_or_else(|| c[0].to_string())
        })
        .into_owned();
    let day_first = opts.day_first_dates;
    s = SLASH_DATE
        .replace_all(&s, |c: &Captures| {
            let (a, b): (u32, u32) = (c[1].parse().unwrap_or(0), c[2].parse().unwrap_or(0));
            let (d, m) = if day_first { (a, b) } else { (b, a) };
            let mut y: u64 = c[3].parse().unwrap_or(0);
            if c[3].len() == 2 {
                y += if y < 50 { 2000 } else { 1900 };
            }
            date_words(d, m, y).unwrap_or_else(|| c[0].to_string())
        })
        .into_owned();
    s = TIME
        .replace_all(&s, |c: &Captures| {
            let h: u64 = c[1].parse().unwrap_or(0);
            let m: u64 = c[2].parse().unwrap_or(0);
            let mins = match m {
                0 => if c.get(3).is_some() { String::new() } else { " hundred".to_string() },
                1..=9 => format!(" oh {}", ONES[m as usize]),
                _ => format!(" {}", number_words(m)),
            };
            let suffix = match c.get(3).map(|x| x.as_str().replace('.', "").to_lowercase()) {
                Some(x) if x == "am" => " a m",
                Some(x) if x == "pm" => " p m",
                _ => "",
            };
            if h > 24 || m > 59 {
                return c[0].to_string();
            }
            format!("{}{}{}", number_words(h), mins, suffix)
        })
        .into_owned();
    s = YEAR_RANGE
        .replace_all(&s, |c: &Captures| {
            let a: u64 = c[1].parse().unwrap_or(0);
            let b_raw = &c[2];
            let b: u64 = if b_raw.len() == 2 { a / 100 * 100 + b_raw.parse::<u64>().unwrap_or(0) } else { b_raw.parse().unwrap_or(0) };
            format!("{} to {}", year_words(a), year_words(b))
        })
        .into_owned();
    s = NUM_RANGE.replace_all(&s, "$1 to $2").into_owned();
    s = ORDINAL
        .replace_all(&s, |c: &Captures| c[1].parse::<u64>().map(ordinal_words).unwrap_or_else(|_| c[0].to_string()))
        .into_owned();

    // 5. Identifiers and acronyms.
    if opts.spell_acronyms {
        s = ALNUM_ID.replace_all(&s, |c: &Captures| spell(&c[0])).into_owned();
        s = ACRONYM
            .replace_all(&s, |c: &Captures| {
                let w = &c[0];
                let (core, plural) = match w.strip_suffix('s') {
                    Some(core) if core.len() >= 2 => (core, true),
                    _ => (w, false),
                };
                if SPOKEN_ACRONYMS.contains(&core) {
                    return w.to_string();
                }
                if should_spell(core) {
                    format!("{}{}", spell(core), if plural { "s" } else { "" })
                } else {
                    w.to_string()
                }
            })
            .into_owned();
    }

    // 6. Remaining numbers. Standalone four-digit years read as years.
    let spell_mode = opts.spell_mode;
    s = NUMBER
        .replace_all(&s, |c: &Captures| {
            let raw = &c[0];
            if spell_mode {
                return digits_words(raw);
            }
            if raw.len() == 4 && !raw.contains(['.', ',']) {
                if let Ok(y) = raw.parse::<u64>() {
                    if (1100..=2099).contains(&y) {
                        return year_words(y);
                    }
                }
            }
            // Long digit strings without separators are identifiers: read digit by digit.
            if raw.len() > 6 && !raw.contains([',', '.']) || raw.starts_with('0') && raw.len() > 1 && !raw.contains('.') {
                return digits_words(raw);
            }
            decimal_words(raw)
        })
        .into_owned();

    // 7. Symbols.
    s = s
        .replace(" & ", " and ")
        .replace('&', " and ")
        .replace('§', " section ")
        .replace(['→', '⇒'], " to ")
        .replace(['•', '▪', '●', '◦', '‣', '■'], " ")
        .replace(['“', '”', '„'], "\"")
        .replace(['‘', '’'], "'")
        .replace('…', "...")
        .replace(" / ", " or ");
    // 8. Rules apply again to words produced above (e.g. "cedis" from "GH₵").
    let s = apply_rules(&s, rules, &mut slots);
    super::extract::clean(&restore_slots(&s, &slots))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(s: &str) -> String {
        normalize(s, &[], &NormalizeOptions::default())
    }

    #[test]
    fn numbers() {
        assert_eq!(number_words(105), "one hundred and five");
        assert_eq!(number_words(1005), "one thousand and five");
        assert_eq!(number_words(1205), "one thousand, two hundred and five");
        assert_eq!(number_words(2_500_000), "two million, five hundred thousand");
        assert_eq!(ordinal_words(21), "twenty-first");
        assert_eq!(ordinal_words(40), "fortieth");
        assert_eq!(year_words(1998), "nineteen ninety-eight");
        assert_eq!(year_words(2005), "two thousand and five");
        assert_eq!(year_words(2026), "twenty twenty-six");
    }

    #[test]
    fn money_and_percent() {
        assert_eq!(n("It cost GH₵ 2.5m."), "It cost two point five million cedis.");
        assert_eq!(n("Fee: GHS 1,250.50"), "Fee: one thousand, two hundred and fifty cedis and fifty pesewas");
        assert_eq!(n("$1 only"), "one dollar only");
        assert_eq!(n("Up 12.5% this year"), "Up twelve point five percent this year");
    }

    #[test]
    fn dates_and_times() {
        assert_eq!(n("Due 26/10/2026."), "Due the twenty-sixth of October twenty twenty-six.");
        assert_eq!(n("on 2026-03-01"), "on the first of March twenty twenty-six");
        assert_eq!(n("at 10:30am"), "at ten thirty a m");
        assert_eq!(n("from 2019-2021"), "from twenty nineteen to twenty twenty-one");
        assert_eq!(n("the 3rd item"), "the third item");
    }

    #[test]
    fn acronyms_ids_and_abbreviations() {
        assert_eq!(n("AML and KYC under FATF"), "A M L and K Y C under F A T F");
        assert_eq!(n("ECOWAS member"), "ECOWAS member");
        assert_eq!(n("NASA and KNUST"), "NASA and K N U S T");
        assert_eq!(n("Student PG4011725"), "Student P G four zero one one seven two five");
        assert_eq!(n("See Fig. 3, e.g. Dr. Osei"), "See Figure three, for example Doctor Osei");
        assert_eq!(n("Email info@ecb.com.gh"), "Email info at ecb dot com dot gh");
        assert_eq!(n("Call 0244123456"), "Call zero two four four one two three four five six");
    }

    #[test]
    fn user_rules_win() {
        let rules = vec![PronunciationRule {
            id: 1,
            term: "Setsoafia".into(),
            replacement: "Set-so-AH-fee-ah".into(),
            case_sensitive: false,
            document_id: None,
        }];
        assert_eq!(
            normalize("Mr. Setsoafia spoke", &rules, &NormalizeOptions::default()),
            "Mister Set-so-AH-fee-ah spoke"
        );
        let cedi = vec![PronunciationRule { id: 2, term: "cedis".into(), replacement: "see-deez".into(), case_sensitive: false, document_id: None }];
        assert_eq!(normalize("GH₵ 20", &cedi, &NormalizeOptions::default()), "twenty see-deez");
    }
}
