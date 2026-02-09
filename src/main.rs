use crossterm::{
    cursor, execute,
    event::{self, Event, KeyCode, KeyModifiers},
    style::{self, Stylize},
    terminal::{self, ClearType},
};
use num_bigint::BigInt;
use num_traits::{One, Zero};
use std::collections::VecDeque;
use std::io::{self, Write};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use std::thread;
use std::time::{Duration, Instant};

// ═══════════════════════════════════════════════════════════════
//  Chudnovsky Binary Splitting  (same algorithm as world records)
// ═══════════════════════════════════════════════════════════════

const CHUD_A: i64 = 13591409;
const CHUD_B: i64 = 545140134;
const C3_OVER_24: i64 = 10_939_058_860_032_000; // 640320³ / 24

/// Binary splitting: returns (P, Q, T) for terms [a, b)
fn binary_split(a: u64, b: u64) -> (BigInt, BigInt, BigInt) {
    if b - a == 1 {
        if a == 0 {
            return (BigInt::one(), BigInt::one(), BigInt::from(CHUD_A));
        }
        let a_bi = BigInt::from(a);
        let p = BigInt::from(6 * a - 5) * BigInt::from(2 * a - 1) * BigInt::from(6 * a - 1);
        let q = &a_bi * &a_bi * &a_bi * BigInt::from(C3_OVER_24);
        let mut t = (BigInt::from(CHUD_A) + BigInt::from(CHUD_B) * a_bi) * &p;
        if a % 2 == 1 {
            t = -t;
        }
        (p, q, t)
    } else {
        let m = (a + b) / 2;
        let (pl, ql, tl) = binary_split(a, m);
        let (pr, qr, tr) = binary_split(m, b);
        (&pl * &pr, &ql * &qr, &qr * tl + &pl * tr)
    }
}

/// Integer square root via Newton's method
fn isqrt(n: &BigInt) -> BigInt {
    if n.is_zero() {
        return BigInt::zero();
    }
    let mut x = BigInt::one() << ((n.bits() + 1) / 2);
    loop {
        let next = (&x + n / &x) >> 1u32;
        if next >= x {
            return x;
        }
        x = next;
    }
}

/// Compute `num_digits` decimal digits of Pi (after the decimal point)
fn compute_pi_digits(num_digits: usize) -> Vec<u8> {
    let extra = 20;
    let total = num_digits + extra;
    let num_terms = (total as f64 / 14.181647) as u64 + 2;

    let (_, q, t) = binary_split(0, num_terms);

    // π × 10^total = Q × 426880 × √(10005 × 10^(2·total)) / T
    let ten_pow = BigInt::from(10u32).pow(2 * total as u32);
    let sqrt_c = isqrt(&(BigInt::from(10005u32) * ten_pow));
    let pi_scaled = q * 426880u32 * sqrt_c / t;

    let s = pi_scaled.to_string();
    s.bytes()
        .skip(1) // skip leading '3'
        .take(num_digits)
        .map(|b| b - b'0')
        .collect()
}

// ═══════════════════════════════════════════════════════════════
//  Statistics Tracker
// ═══════════════════════════════════════════════════════════════

struct Stats {
    counts: [u64; 10],
    total: u64,
    first_digits: Vec<u8>,  // permanent: first N digits for correct "Pi = 3.xxx" display
    recent_digits: Vec<u8>, // rolling: latest digits for live feed
    max_dev_history: Vec<f64>,
    entropy_history: Vec<f64>,
    chi_sq_history: Vec<f64>,
    start: Instant,
}

impl Stats {
    fn new() -> Self {
        Self {
            counts: [0; 10],
            total: 0,
            first_digits: Vec::with_capacity(200),
            recent_digits: Vec::with_capacity(600),
            max_dev_history: Vec::new(),
            entropy_history: Vec::new(),
            chi_sq_history: Vec::new(),
            start: Instant::now(),
        }
    }

    fn add_digit(&mut self, d: u8) {
        self.counts[d as usize] += 1;
        self.total += 1;
        // Keep first 200 digits permanently for correct "Pi = 3.xxx" display
        if self.first_digits.len() < 200 {
            self.first_digits.push(d);
        }
        self.recent_digits.push(d);
        if self.recent_digits.len() > 500 {
            self.recent_digits.drain(..200);
        }

        // Sample convergence at adaptive intervals
        let interval = match self.total {
            0..=999 => 50,
            1_000..=9_999 => 200,
            10_000..=99_999 => 1_000,
            _ => 5_000,
        };
        if self.total % interval == 0 {
            self.max_dev_history.push(self.max_deviation());
            self.entropy_history.push(self.entropy());
            self.chi_sq_history.push(self.chi_squared());
            // Decimate if too long
            if self.max_dev_history.len() > 300 {
                self.max_dev_history = self.max_dev_history.iter().step_by(2).copied().collect();
                self.entropy_history = self.entropy_history.iter().step_by(2).copied().collect();
                self.chi_sq_history = self.chi_sq_history.iter().step_by(2).copied().collect();
            }
        }
    }

    fn chi_squared(&self) -> f64 {
        if self.total == 0 {
            return 0.0;
        }
        let exp = self.total as f64 / 10.0;
        self.counts
            .iter()
            .map(|&c| {
                let d = c as f64 - exp;
                d * d / exp
            })
            .sum()
    }

    fn entropy(&self) -> f64 {
        if self.total == 0 {
            return 0.0;
        }
        let t = self.total as f64;
        -self.counts
            .iter()
            .map(|&c| {
                if c == 0 {
                    return 0.0;
                }
                let p = c as f64 / t;
                p * p.log2()
            })
            .sum::<f64>()
    }

    fn max_deviation(&self) -> f64 {
        if self.total == 0 {
            return 0.0;
        }
        self.counts
            .iter()
            .map(|&c| (c as f64 / self.total as f64 * 100.0 - 10.0).abs())
            .fold(0.0f64, f64::max)
    }

    fn speed(&self) -> f64 {
        let elapsed = self.start.elapsed().as_secs_f64();
        if elapsed > 0.0 {
            self.total as f64 / elapsed
        } else {
            0.0
        }
    }
}

// ═══════════════════════════════════════════════════════════════
//  Sparkline
// ═══════════════════════════════════════════════════════════════

fn sparkline(values: &[f64], max_width: usize) -> String {
    if values.is_empty() {
        return String::new();
    }
    let display = if values.len() > max_width {
        &values[values.len() - max_width..]
    } else {
        values
    };
    let max_val = display.iter().copied().fold(0.0f64, f64::max).max(0.001);
    let blocks = ['\u{2581}', '\u{2582}', '\u{2583}', '\u{2584}', '\u{2585}', '\u{2586}', '\u{2587}', '\u{2588}'];
    display
        .iter()
        .map(|&v| {
            let idx = ((v / max_val) * 7.0).round() as usize;
            blocks[idx.min(7)]
        })
        .collect()
}

fn fmt_num(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

// ═══════════════════════════════════════════════════════════════
//  Display  (flicker-free: overwrite in place, clear to EOL)
// ═══════════════════════════════════════════════════════════════

const BAR_COLORS: [style::Color; 10] = [
    style::Color::Red,
    style::Color::Green,
    style::Color::Yellow,
    style::Color::Blue,
    style::Color::Magenta,
    style::Color::Cyan,
    style::Color::White,
    style::Color::DarkRed,
    style::Color::DarkGreen,
    style::Color::DarkYellow,
];

fn draw(stdout: &mut io::Stdout, stats: &Stats, first: &mut bool) -> io::Result<()> {
    let (tw, _) = terminal::size().unwrap_or((80, 24));
    let w = tw as usize;

    if *first {
        execute!(stdout, terminal::Clear(ClearType::All))?;
        *first = false;
    }

    let sep: String = "\u{2500}".repeat(w);

    // Row 0: Title
    let title = format!(
        "  Pi Normal Number Test \u{2014} {} digits ({:.0} d/s)     [Chudnovsky Binary Splitting]",
        fmt_num(stats.total),
        stats.speed(),
    );
    execute!(
        stdout,
        cursor::MoveTo(0, 0),
        style::PrintStyledContent(title.bold()),
        terminal::Clear(ClearType::UntilNewLine),
    )?;

    // Row 1: Separator
    execute!(
        stdout,
        cursor::MoveTo(0, 1),
        style::Print(&sep),
        terminal::Clear(ClearType::UntilNewLine),
    )?;

    // Row 2–11: Bar chart (digits 0-9)
    let max_count = stats.counts.iter().copied().max().unwrap_or(1).max(1);
    let bar_max = w.saturating_sub(36).max(10);

    for (i, &count) in stats.counts.iter().enumerate() {
        let pct = if stats.total > 0 {
            count as f64 / stats.total as f64 * 100.0
        } else {
            0.0
        };
        let dev = pct - 10.0;
        let bar_len = (count as f64 / max_count as f64 * bar_max as f64) as usize;
        let bar: String = "\u{2588}".repeat(bar_len);

        execute!(
            stdout,
            cursor::MoveTo(0, i as u16 + 2),
            style::Print(format!("  {} \u{2502} ", i)),
            style::PrintStyledContent(bar.with(BAR_COLORS[i])),
            style::Print(format!(
                " {:>8} ({:>5.2}% {:>+6.2}%)",
                fmt_num(count),
                pct,
                dev
            )),
            terminal::Clear(ClearType::UntilNewLine),
        )?;
    }

    // Row 12: Separator
    execute!(
        stdout,
        cursor::MoveTo(0, 12),
        style::Print(&sep),
        terminal::Clear(ClearType::UntilNewLine),
    )?;

    // Row 13: Pi = 3.xxxxx (always correct first digits)
    let first: String = stats.first_digits.iter().map(|d| (b'0' + d) as char).collect();
    let dw = w.saturating_sub(14);
    let pi_shown = if first.len() > dw {
        &first[..dw]
    } else {
        &first
    };
    let ellipsis = if stats.total as usize > stats.first_digits.len() { "..." } else { "" };
    execute!(
        stdout,
        cursor::MoveTo(0, 13),
        style::PrintStyledContent("  Pi = 3.".bold()),
        style::Print(pi_shown),
        style::PrintStyledContent(ellipsis.dark_grey()),
        terminal::Clear(ClearType::UntilNewLine),
    )?;

    // Row 14: Latest computed digits (live feed)
    let recent: String = stats.recent_digits.iter().map(|d| (b'0' + d) as char).collect();
    let rw = w.saturating_sub(16);
    let recent_shown = if recent.len() > rw {
        &recent[recent.len() - rw..]
    } else {
        &recent
    };
    execute!(
        stdout,
        cursor::MoveTo(0, 14),
        style::PrintStyledContent("  Latest: ...".dark_grey()),
        style::Print(recent_shown),
        terminal::Clear(ClearType::UntilNewLine),
    )?;

    // Row 15: Separator
    execute!(
        stdout,
        cursor::MoveTo(0, 15),
        style::Print(&sep),
        terminal::Clear(ClearType::UntilNewLine),
    )?;

    // Row 16: Statistics line
    if stats.total > 0 {
        let chi = stats.chi_squared();
        let ent = stats.entropy();
        let max_ent = 10.0f64.log2();
        let ent_pct = ent / max_ent * 100.0;
        let max_dev = stats.max_deviation();

        let chi_label = if chi < 16.919 {
            "UNIFORM".with(style::Color::Green)
        } else {
            "SKEWED".with(style::Color::Yellow)
        };

        execute!(
            stdout,
            cursor::MoveTo(0, 16),
            style::PrintStyledContent("  \u{03C7}\u{00B2}= ".bold()),
            style::Print(format!("{:<8.3} ", chi)),
            style::PrintStyledContent(chi_label),
            style::Print(format!(
                "   Entropy: {:.4}/{:.4} bits ({:.2}%)   |dev|max: {:.3}%",
                ent, max_ent, ent_pct, max_dev
            )),
            terminal::Clear(ClearType::UntilNewLine),
        )?;
    }

    // Row 18: Convergence sparkline — max deviation
    let spark_w = w.saturating_sub(38).max(10);
    let spark_dev = sparkline(&stats.max_dev_history, spark_w);
    execute!(
        stdout,
        cursor::MoveTo(0, 18),
        style::PrintStyledContent("  Max |deviation| \u{2192} 0 : ".dark_grey()),
        style::PrintStyledContent(spark_dev.with(style::Color::Cyan)),
        terminal::Clear(ClearType::UntilNewLine),
    )?;

    // Row 19: Convergence sparkline — entropy
    let spark_ent = sparkline(&stats.entropy_history, spark_w);
    execute!(
        stdout,
        cursor::MoveTo(0, 19),
        style::PrintStyledContent("  Entropy \u{2192} 3.3219 : ".dark_grey()),
        style::PrintStyledContent(spark_ent.with(style::Color::Green)),
        terminal::Clear(ClearType::UntilNewLine),
    )?;

    // Row 20: Convergence sparkline — chi-squared
    let spark_chi = sparkline(&stats.chi_sq_history, spark_w);
    execute!(
        stdout,
        cursor::MoveTo(0, 20),
        style::PrintStyledContent("  \u{03C7}\u{00B2} \u{2192} 0          : ".dark_grey()),
        style::PrintStyledContent(spark_chi.with(style::Color::Yellow)),
        terminal::Clear(ClearType::UntilNewLine),
    )?;

    // Row 22: Controls
    execute!(
        stdout,
        cursor::MoveTo(0, 22),
        style::PrintStyledContent("  Press Ctrl+C or ESC to stop".dark_grey()),
        terminal::Clear(ClearType::UntilNewLine),
    )?;

    stdout.flush()
}

// ═══════════════════════════════════════════════════════════════
//  Main
// ═══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_pi_first_50() {
        let digits = compute_pi_digits(50);
        let s: String = digits.iter().map(|d| (b'0' + d) as char).collect();
        // Pi = 3.14159265358979323846264338327950288419716939937510
        assert_eq!(s, "14159265358979323846264338327950288419716939937510");
    }
}

fn main() -> io::Result<()> {
    let mut stdout = io::stdout();
    terminal::enable_raw_mode()?;
    execute!(stdout, terminal::EnterAlternateScreen, cursor::Hide)?;

    let result = run(&mut stdout);

    execute!(stdout, cursor::Show, terminal::LeaveAlternateScreen)?;
    terminal::disable_raw_mode()?;
    result
}

fn run(stdout: &mut io::Stdout) -> io::Result<()> {
    let running = Arc::new(AtomicBool::new(true));

    // Keyboard handler
    {
        let r = running.clone();
        thread::spawn(move || loop {
            if event::poll(Duration::from_millis(100)).unwrap_or(false) {
                if let Ok(Event::Key(key)) = event::read() {
                    match key.code {
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            r.store(false, Ordering::Relaxed);
                            return;
                        }
                        KeyCode::Esc => {
                            r.store(false, Ordering::Relaxed);
                            return;
                        }
                        _ => {}
                    }
                }
            }
        });
    }

    // Pi computation thread — growing batches via Chudnovsky
    let (tx, rx) = mpsc::sync_channel::<Vec<u8>>(2);
    {
        let r = running.clone();
        thread::spawn(move || {
            let mut computed = 0usize;
            let mut target = 1_000usize;
            while r.load(Ordering::Relaxed) {
                let all = compute_pi_digits(target);
                let new_digits = all[computed..].to_vec();
                if tx.send(new_digits).is_err() {
                    break;
                }
                computed = target;
                target = (target * 2).min(2_000_000);
            }
        });
    }

    let mut stats = Stats::new();
    let mut first_draw = true;
    let mut last_draw = Instant::now();
    let mut digit_buf: VecDeque<u8> = VecDeque::new();

    while running.load(Ordering::Relaxed) {
        // Receive computed digits (non-blocking)
        while let Ok(batch) = rx.try_recv() {
            digit_buf.extend(batch);
        }

        // Process all buffered digits into stats
        while let Some(d) = digit_buf.pop_front() {
            stats.add_digit(d);
        }

        // Throttled draw (50ms = ~20fps)
        if last_draw.elapsed() >= Duration::from_millis(50) {
            draw(stdout, &stats, &mut first_draw)?;
            last_draw = Instant::now();
        }

        thread::sleep(Duration::from_millis(1));
    }

    // Final draw
    draw(stdout, &stats, &mut first_draw)?;
    thread::sleep(Duration::from_secs(1));
    Ok(())
}
