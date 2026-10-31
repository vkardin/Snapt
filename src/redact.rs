//! Finding personal data in recognized text: e-mail addresses, phone and card
//! numbers, IBANs and IP addresses, so the editor can blur them in one go.

use crate::ocr::Word;

/// A box to hide, in the pixels of the recognized image.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Area {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// The areas of `words` that hold personal data, padded a little.
pub fn find(words: &[Word]) -> Vec<Area> {
    let mut areas = Vec::new();
    let mut start = 0;
    while start < words.len() {
        let line = words[start].line;
        let end = (start..words.len())
            .find(|&i| words[i].line != line)
            .unwrap_or(words.len());
        find_in_line(&words[start..end], &mut areas);
        start = end;
    }
    areas
}

fn find_in_line(words: &[Word], areas: &mut Vec<Area>) {
    let mut i = 0;
    while i < words.len() {
        let word = trim(&words[i].text);
        if is_email(word) || is_ip(word) {
            areas.push(cover(&words[i..=i]));
            i += 1;
            continue;
        }
        // Numbers are often split by spaces: "+7 999 123-45-67", "4111 1111 1111 1111".
        let run = words[i..]
            .iter()
            .take_while(|w| is_number_part(trim(&w.text)))
            .count();
        if run > 0 {
            let text: String = words[i..i + run].iter().map(|w| trim(&w.text)).collect();
            if is_long_number(&text) {
                areas.push(cover(&words[i..i + run]));
                i += run;
                continue;
            }
        }
        let run = iban_run(&words[i..]);
        if run > 0 {
            areas.push(cover(&words[i..i + run]));
            i += run;
            continue;
        }
        i += 1;
    }
}

/// Drops punctuation that sticks to a word in running text.
fn trim(word: &str) -> &str {
    word.trim_matches(|c: char| matches!(c, ',' | ';' | ':' | '"' | '\'' | '<' | '>' | '«' | '»'))
        .trim_end_matches(['.', '!', '?'])
}

fn is_email(word: &str) -> bool {
    let Some((name, domain)) = word.split_once('@') else {
        return false;
    };
    let ok = |c: char| c.is_alphanumeric() || matches!(c, '.' | '_' | '-' | '+');
    !name.is_empty()
        && name.chars().all(ok)
        && domain.chars().all(ok)
        && domain
            .rsplit_once('.')
            .is_some_and(|(host, tld)| !host.is_empty() && tld.chars().count() >= 2)
}

fn is_ip(word: &str) -> bool {
    let parts: Vec<&str> = word.split('.').collect();
    parts.len() == 4
        && parts.iter().all(|p| {
            !p.is_empty() && p.len() <= 3 && p.chars().all(|c| c.is_ascii_digit())
        })
        && parts.iter().all(|p| p.parse::<u16>().is_ok_and(|n| n <= 255))
        // Version numbers like 1.2.3.4 are rare on screen; leading zeros mean something else.
        && !parts.iter().all(|p| p.len() == 1)
}

fn is_number_part(word: &str) -> bool {
    !word.is_empty()
        && word.chars().any(|c| c.is_ascii_digit())
        && word
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '+' | '(' | ')' | '-' | '.' | '/'))
}

/// Phone, card and account numbers: at least ten digits, or a `+` and eight.
/// Dates and times are shorter, so they stay.
fn is_long_number(text: &str) -> bool {
    let digits = text.chars().filter(char::is_ascii_digit).count();
    let international = text.starts_with('+') && digits >= 8;
    (digits >= 10 || international) && digits <= 34
}

/// How many words from the start make up an IBAN, such as `DE89 3704 0044 0532 0130 00`.
fn iban_run(words: &[Word]) -> usize {
    let first = trim(&words[0].text);
    let mut chars = first.chars();
    let starts = chars.next().is_some_and(|c| c.is_ascii_uppercase())
        && chars.next().is_some_and(|c| c.is_ascii_uppercase())
        && chars.next().is_some_and(|c| c.is_ascii_digit())
        && chars.next().is_some_and(|c| c.is_ascii_digit());
    if !starts {
        return 0;
    }
    let mut len = 0;
    let mut run = 0;
    for word in words {
        let text = trim(&word.text);
        if !text
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
            || len >= 34
        {
            break;
        }
        len += text.len();
        run += 1;
    }
    if (15..=34).contains(&len) { run } else { 0 }
}

/// One box around `words`, with a margin so no letter edges show.
fn cover(words: &[Word]) -> Area {
    let left = words.iter().map(|w| w.x).fold(f32::MAX, f32::min);
    let top = words.iter().map(|w| w.y).fold(f32::MAX, f32::min);
    let right = words.iter().map(|w| w.x + w.w).fold(f32::MIN, f32::max);
    let bottom = words.iter().map(|w| w.y + w.h).fold(f32::MIN, f32::max);
    let pad = ((bottom - top) * 0.2).max(2.0);
    Area {
        x: left - pad,
        y: top - pad,
        w: right - left + 2.0 * pad,
        h: bottom - top + 2.0 * pad,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Words laid out left to right, 10 px per character, one line per string.
    fn words(lines: &[&str]) -> Vec<Word> {
        let mut out = Vec::new();
        for (line, text) in lines.iter().enumerate() {
            let mut x = 0.0;
            for word in text.split(' ') {
                let w = word.chars().count() as f32 * 10.0;
                out.push(Word {
                    text: word.into(),
                    line,
                    x,
                    y: line as f32 * 30.0,
                    w,
                    h: 20.0,
                });
                x += w + 10.0;
            }
        }
        out
    }

    /// The text under each found area.
    fn hidden(lines: &[&str]) -> Vec<String> {
        let words = words(lines);
        find(&words)
            .iter()
            .map(|a| {
                words
                    .iter()
                    .filter(|w| {
                        w.x >= a.x && w.x + w.w <= a.x + a.w && w.y >= a.y && w.y + w.h <= a.y + a.h
                    })
                    .map(|w| w.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect()
    }

    #[test]
    fn emails_are_found() {
        assert_eq!(
            hidden(&["Write to ivan.petrov@mail.ru, please"]),
            ["ivan.petrov@mail.ru,"]
        );
        assert!(hidden(&["@channel and user@ and a@b"]).is_empty());
    }

    #[test]
    fn phones_split_by_spaces_are_found_whole() {
        assert_eq!(
            hidden(&["Call +7 (999) 123-45-67 today"]),
            ["+7 (999) 123-45-67"]
        );
        assert_eq!(hidden(&["Tel: 8 800 555 35 35"]), ["8 800 555 35 35"]);
    }

    #[test]
    fn card_numbers_and_ibans_are_found() {
        assert_eq!(
            hidden(&["Card 4111 1111 1111 1111 exp 12/27"]),
            ["4111 1111 1111 1111"]
        );
        assert_eq!(
            hidden(&["IBAN DE89 3704 0044 0532 0130 00"]),
            ["DE89 3704 0044 0532 0130 00"]
        );
    }

    #[test]
    fn ip_addresses_are_found() {
        assert_eq!(
            hidden(&["Server at 192.168.10.25 is up"]),
            ["192.168.10.25"]
        );
        assert!(hidden(&["Version 300.1.2.3"]).is_empty());
    }

    #[test]
    fn dates_times_and_prices_stay() {
        assert!(hidden(&["31.10.2026 16:45 total 1 299.00 order 12345"]).is_empty());
    }

    #[test]
    fn numbers_do_not_run_across_lines() {
        assert!(hidden(&["Room 12345", "67890 more"]).is_empty());
    }
}

/// Reads real text with the system's OCR; skipped where there is none, as in CI.
#[cfg(test)]
mod with_ocr {
    #[test]
    fn an_email_on_screen_is_found_and_covered() {
        let mut pixmap = tiny_skia::Pixmap::new(520, 90).unwrap();
        pixmap.fill(tiny_skia::Color::WHITE);
        crate::font::draw(
            &mut pixmap,
            "Mail ivan.petrov@example.com now",
            10.0,
            10.0,
            22.0,
            0x202020,
            1.0,
        );
        crate::font::draw(
            &mut pixmap,
            "Phone +7 999 123-45-67",
            10.0,
            50.0,
            22.0,
            0x202020,
            1.0,
        );
        let image = image::RgbaImage::from_raw(520, 90, pixmap.take()).unwrap();
        let words = match crate::ocr::words(&image) {
            Ok(words) => words,
            Err(err) => {
                eprintln!("skipped: no OCR here ({err:?})");
                return;
            }
        };
        let areas = super::find(&words);
        assert_eq!(areas.len(), 2, "{words:?}");
        // The e-mail starts after "Mail " and the box reaches past its end.
        assert!(
            areas[0].x > 40.0 && areas[0].x + areas[0].w > 250.0,
            "{areas:?}"
        );
        assert!(areas[1].y > 40.0, "{areas:?}");
    }
}
