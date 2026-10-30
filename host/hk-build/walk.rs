//! Walk scripts: a route written as goals, compiled to a `psoxide-gate` journey.
//!
//! A walk script drives the pad from game state, the way hl-psx's chain does, and plays no
//! recorded input. Every step either holds buttons until a RAM word crosses a value
//! (`hold_until`), waits on one (`wait_until`), or presses for a fixed handful of ticks (a
//! single tap or swing; nothing in a walk script is timed through a load or a fight). The gate
//! plays the steps in order, so a walk is a straight line of goals with tolerances: a hold
//! whose condition already holds ends at once, which is what lets a goal be written twice
//! (a dead band) and what makes every goal safe to repeat.
//!
//! One directive a line; `#` starts a comment, and the comment after a directive is its label
//! in the report. Positions are world units (`x 12.5`), converted to the Q16 words the game
//! publishes.
//!
//! ```text
//! name       NAME                        journey id (required, first)
//! title      TEXT                        report title
//! max_ticks  N                           hard stop
//! sym        ALIAS SYMBOL [signed]       another name for a telemetry word
//! wait       N                           N ticks of rest
//! wait       until COND [timeout N]      run until the condition holds
//! press      BUTTONS [for N] [rest N]    tap (3 ticks, no rest unless told)
//! hold       BUTTONS until COND [timeout N]
//! hold       BUTTONS for N               hold for N ticks
//! repeat     N press BUTTONS [for H] [rest R]
//! probe      NAME WORD [WORD...]         checkpoint that reports each word (never passes)
//! check      NAME COND [; COND...]       checkpoint whose conditions must hold
//! snapshot                               hand the memory card to the next journey
//! raw        TOML                        one verbatim line of step TOML (rare)
//! ```
//!
//! `BUTTONS` are the gate's names joined by `+` (`right+cross`). `COND` is `WORD OP VALUE`,
//! or `WORD between LO HI`; `OP` is `eq ne lt le gt ge` (or `== != < <= > >=`). `WORD` is
//! `x`, `y`, a `sym` alias, or any `HK_` telemetry symbol. A value on `x` or `y` is in world
//! units and may be fractional; any other value is an integer.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

type Result<T> = std::result::Result<T, String>;

/// One world unit in the Q16 words the game publishes.
const ONE: i64 = 65_536;
/// A hold waits this long for its condition unless the line says otherwise (60 ticks a second).
const DEFAULT_TIMEOUT: u64 = 1_800;

/// How far the Knight walks in one tick (a speed of 8.3 units a second, a tick a sixtieth).
const WALK_PER_TICK: f64 = 0.138;
/// How far before its take-off a leap lets go of the walk (see `macros`).
const LEAP_EARLY: f64 = 0.3;

/// What the compiled journey boots.
pub struct Setup<'a> {
    pub disc: &'a Path,
    pub symbols: &'a Path,
}

#[derive(Clone)]
struct Word {
    symbol: String,
    /// The word is in world units on the script's side (a Q16 position).
    units: bool,
    signed: bool,
}

struct Cond {
    word: Word,
    op: &'static str,
    value: i64,
    high: Option<i64>,
}

fn alias_table() -> BTreeMap<String, Word> {
    let mut table = BTreeMap::new();
    for (alias, symbol) in [("x", "HK_PLAYER_X"), ("y", "HK_PLAYER_Y")] {
        table.insert(
            alias.to_string(),
            Word { symbol: symbol.to_string(), units: true, signed: true },
        );
    }
    table
}

fn operator(text: &str) -> Option<&'static str> {
    Some(match text {
        "eq" | "==" | "=" => "eq",
        "ne" | "!=" => "ne",
        "lt" | "<" => "lt",
        "le" | "<=" => "le",
        "gt" | ">" => "gt",
        "ge" | ">=" => "ge",
        "between" => "between",
        _ => return None,
    })
}

/// A value as the gate wants it: world units become Q16, anything else stays an integer.
fn value(text: &str, units: bool) -> Result<i64> {
    if units {
        let (negative, digits) = match text.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, text),
        };
        let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
        let whole: i64 = whole.parse().map_err(|_| format!("bad number `{text}`"))?;
        let mut scaled = whole * ONE;
        if !fraction.is_empty() {
            let numerator: i64 = fraction.parse().map_err(|_| format!("bad number `{text}`"))?;
            let denominator = 10i64.pow(fraction.len() as u32);
            scaled += (numerator * ONE + denominator / 2) / denominator;
        }
        Ok(if negative { -scaled } else { scaled })
    } else {
        text.replace('_', "").parse().map_err(|_| format!("bad number `{text}`"))
    }
}

fn parse_cond(tokens: &[&str], words: &BTreeMap<String, Word>) -> Result<Cond> {
    let [name, op, rest @ ..] = tokens else {
        return Err(format!("a condition is WORD OP VALUE, got `{}`", tokens.join(" ")));
    };
    let word = words.get(*name).cloned().unwrap_or_else(|| Word {
        symbol: name.to_string(),
        units: false,
        signed: false,
    });
    let op = operator(op).ok_or(format!("unknown operator `{op}`"))?;
    let (low, high) = match (op, rest) {
        ("between", [low, high]) => (value(low, word.units)?, Some(value(high, word.units)?)),
        ("between", _) => return Err("between takes LO HI".into()),
        (_, [one]) => (value(one, word.units)?, None),
        _ => return Err(format!("`{}` takes one value", op)),
    };
    let signed = word.signed || low < 0 || high.is_some_and(|h| h < 0);
    Ok(Cond { word: Word { signed, ..word }, op, value: low, high })
}

fn write_cond(out: &mut String, table: &str, cond: &Cond, note: Option<&str>) {
    let _ = writeln!(out, "[{table}]\nkind = \"ram\"\nsym = \"{}\"", cond.word.symbol);
    if cond.word.signed {
        let _ = writeln!(out, "signed = true");
    }
    let _ = writeln!(out, "op = \"{}\"\nvalue = {}", cond.op, cond.value);
    if let Some(high) = cond.high {
        let _ = writeln!(out, "value_hi = {high}");
    }
    if let Some(note) = note {
        let _ = writeln!(out, "note = \"{}\"", escape(note));
    }
}

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "'")
}

fn buttons(text: &str) -> Result<String> {
    const NAMES: [&str; 16] = [
        "cross", "circle", "square", "triangle", "start", "select", "up", "down", "left", "right",
        "l1", "r1", "l2", "r2", "l3", "r3",
    ];
    let mut names = Vec::new();
    for name in text.split('+') {
        if !NAMES.contains(&name) {
            return Err(format!("unknown button `{name}`"));
        }
        names.push(format!("\"{name}\""));
    }
    Ok(names.join(", "))
}

/// `key N` pairs after a directive's fixed words: `for 24 rest 10 timeout 900`.
fn options(tokens: &[&str]) -> Result<BTreeMap<String, u64>> {
    let mut found = BTreeMap::new();
    let mut at = 0;
    while at < tokens.len() {
        let key = tokens[at];
        let number = tokens.get(at + 1).ok_or(format!("`{key}` needs a number"))?;
        let number = number.parse::<u64>().map_err(|_| format!("bad number `{number}`"))?;
        found.insert(key.to_string(), number);
        at += 2;
    }
    Ok(found)
}

/// Compile a walk script into the TOML journey for the disc in `setup`.
pub fn compile(text: &str, setup: &Setup) -> Result<String> {
    let text = macros(text)?;
    let text = text.as_str();
    let mut words = alias_table();
    let mut name = None;
    let mut title = None;
    let mut max_ticks = 3_000_000u64;
    let mut body = String::new();
    for (index, line) in text.lines().enumerate() {
        let located = |e: String| format!("line {}: {e}", index + 1);
        let (code, comment) = match line.split_once('#') {
            Some((code, comment)) => (code.trim(), comment.trim()),
            None => (line.trim(), ""),
        };
        let tokens: Vec<&str> = code.split_whitespace().collect();
        let Some((directive, rest)) = tokens.split_first() else {
            continue;
        };
        let label = if comment.is_empty() { code } else { comment };
        let step = |out: &mut String| {
            let _ = writeln!(out, "\n[[step]]\nlabel = \"{}\"", escape(label));
        };
        match *directive {
            "name" => name = rest.first().map(|s| s.to_string()),
            "title" => title = Some(rest.join(" ")),
            "max_ticks" => {
                max_ticks = rest.first().and_then(|n| n.parse().ok()).ok_or(located("max_ticks N".into()))?
            }
            "sym" => {
                let [alias, symbol, flags @ ..] = rest else {
                    return Err(located("sym ALIAS SYMBOL [signed]".into()));
                };
                let word = Word {
                    symbol: symbol.to_string(),
                    units: false,
                    signed: flags.contains(&"signed"),
                };
                words.insert(alias.to_string(), word);
            }
            "wait" => {
                step(&mut body);
                match rest {
                    ["until", cond @ ..] => {
                        let at = cond.iter().position(|t| *t == "timeout");
                        let (cond, opts) = cond.split_at(at.unwrap_or(cond.len()));
                        let timeout = options(opts).map_err(located)?.get("timeout").copied();
                        let cond = parse_cond(cond, &words).map_err(located)?;
                        let _ = writeln!(body, "timeout = {}", timeout.unwrap_or(DEFAULT_TIMEOUT));
                        write_cond(&mut body, "step.wait_until", &cond, None);
                    }
                    [ticks] => {
                        let ticks = ticks.parse::<u64>().map_err(|_| located("wait N".into()))?;
                        let _ = writeln!(body, "wait = {ticks}");
                    }
                    _ => return Err(located("wait N | wait until COND".into())),
                }
            }
            "hold" => {
                let [pad, tail @ ..] = rest else {
                    return Err(located("hold BUTTONS until COND | for N".into()));
                };
                let pad = buttons(pad).map_err(located)?;
                step(&mut body);
                match tail {
                    ["until", cond @ ..] => {
                        let at = cond.iter().position(|t| *t == "timeout");
                        let (cond, opts) = cond.split_at(at.unwrap_or(cond.len()));
                        let timeout = options(opts).map_err(located)?.get("timeout").copied();
                        let cond = parse_cond(cond, &words).map_err(located)?;
                        let _ = writeln!(body, "press = [{pad}]");
                        let _ = writeln!(body, "timeout = {}", timeout.unwrap_or(DEFAULT_TIMEOUT));
                        write_cond(&mut body, "step.hold_until", &cond, None);
                    }
                    ["for", ticks] => {
                        let ticks = ticks.parse::<u64>().map_err(|_| located("for N".into()))?;
                        let _ = writeln!(body, "press = [{pad}]\nhold = {ticks}");
                    }
                    _ => return Err(located("hold BUTTONS until COND | for N".into())),
                }
            }
            "press" | "repeat" => {
                let (count, rest) = if *directive == "repeat" {
                    let [n, "press", rest @ ..] = rest else {
                        return Err(located("repeat N press BUTTONS [for H] [rest R]".into()));
                    };
                    (n.parse::<u64>().map_err(|_| located("repeat N".into()))?, rest)
                } else {
                    (1, rest)
                };
                let [pad, opts @ ..] = rest else {
                    return Err(located("press BUTTONS [for H] [rest R]".into()));
                };
                let pad = buttons(pad).map_err(located)?;
                let opts = options_of(opts, &[("for", "for"), ("rest", "rest")]).map_err(located)?;
                step(&mut body);
                let _ = writeln!(body, "press = [{pad}]");
                let _ = writeln!(body, "hold = {}", opts.get("for").copied().unwrap_or(3));
                if let Some(rest) = opts.get("rest") {
                    let _ = writeln!(body, "wait = {rest}");
                }
                if count > 1 {
                    let _ = writeln!(body, "repeat = {count}");
                }
            }
            "probe" => {
                let [checkpoint, probes @ ..] = rest else {
                    return Err(located("probe NAME WORD...".into()));
                };
                step(&mut body);
                let _ = writeln!(body, "checkpoint = \"{checkpoint}\"\ngolden = false\nno_matrix = true");
                for probe in probes {
                    let cond = parse_cond(&[probe, "eq", "-1"], &words).map_err(located);
                    let mut cond = cond?;
                    cond.word.signed = true;
                    // The word is read in its own units: report raw words, not world units.
                    write_cond(&mut body, "[step.assert]", &cond, Some(&format!("probe {probe}")));
                }
            }
            "check" => {
                let [checkpoint, conds @ ..] = rest else {
                    return Err(located("check NAME COND [; COND...]".into()));
                };
                step(&mut body);
                let _ = writeln!(body, "checkpoint = \"{checkpoint}\"\ngolden = false\nno_matrix = true");
                for cond in conds.split(|t| *t == ";") {
                    let parsed = parse_cond(cond, &words).map_err(located)?;
                    write_cond(&mut body, "[step.assert]", &parsed, Some(&cond.join(" ")));
                }
            }
            "snapshot" => {
                step(&mut body);
                let _ = writeln!(body, "[step.card]\naction = \"snapshot\"");
            }
            "raw" => {
                let _ = writeln!(body, "{}", rest.join(" "));
            }
            other => return Err(located(format!("unknown directive `{other}`"))),
        }
    }
    let name = name.ok_or("a walk script starts with `name NAME`")?;
    let title = title.unwrap_or_else(|| name.clone());
    let mut toml = String::new();
    let _ = writeln!(toml, "# Generated by cargo hk-build walk from a walk script (docs/ROUTES.md, Walks).");
    let _ = writeln!(toml, "name = \"{}\"\ntitle = \"{}\"", escape(&name), escape(&title));
    let _ = writeln!(toml, "disc = [\"{}\"]", setup.disc.display());
    let _ = writeln!(toml, "symbols = [\"{}\"]\nmax_ticks = {max_ticks}", setup.symbols.display());
    toml.push_str(&body);
    Ok(toml)
}

/// The runnable walk scripts of `tools/walks`: the files whose first line of code is `name NAME`
/// (the others are fragments another script includes). Sorted by name.
pub fn scripts(root: &Path) -> Result<Vec<(String, std::path::PathBuf)>> {
    let dir = root.join("tools/walks");
    let mut found = Vec::new();
    let entries = std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "walk") {
            continue;
        }
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let first = text.lines().map(|l| l.split('#').next().unwrap_or("").trim()).find(|l| !l.is_empty());
        if let Some(name) = first.and_then(|l| l.strip_prefix("name ")) {
            found.push((name.trim().to_string(), path));
        }
    }
    found.sort();
    Ok(found)
}

/// Play one walk script on the disc in `library` with `gate`, writing the journey and the gate's
/// report under `out`. Returns the gate's verdict line and whether it passed.
pub fn play(gate: &Path, library: &Path, map: &Path, out: &Path, name: &str, script: &Path) -> Result<(bool, String)> {
    let dir = out.join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let cue = crate::disc::disc_cue(library);
    let toml = compile(&expand(script)?, &Setup { disc: &cue, symbols: map })?;
    let file = dir.join(format!("{name}.toml"));
    std::fs::write(&file, toml).map_err(|e| e.to_string())?;
    let output = std::process::Command::new(gate)
        .current_dir(&dir)
        .arg("run")
        .arg(&file)
        .arg("--disc")
        .arg(&cue)
        .args(["--no-hw", "--out"])
        .arg(dir.join("report"))
        .output()
        .map_err(|e| format!("{}: {e}", gate.display()))?;
    let text = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    std::fs::write(dir.join("gate.log"), &text).map_err(|e| e.to_string())?;
    let headline = text
        .lines()
        .find(|l| l.starts_with("PASS ") || l.starts_with("FAIL ") || l.starts_with("ABORT "))
        .unwrap_or("no verdict from psoxide-gate")
        .to_string();
    let failures = text.lines().any(|l| l.trim_start().starts_with("FAIL ") && !l.starts_with("FAIL "));
    Ok((output.status.success() && headline.starts_with("PASS ") && !failures, headline))
}

/// A position in world units, written as the gate's two-decimal text.
fn units(value: f64) -> String {
    format!("{value:.2}")
}

/// Swing at a foe on `side` if one is inside the swing's box and the nail is ready, then wait for the
/// nail. Both steps take no time when there is nothing to strike: the guest publishes the word that
/// holds the button (`HK_WALK_STRIKE_*`) as zero once the swing has started or there is no foe.
fn strike_one(out: &mut String, label: &str, side: &str) {
    let word = if side == "left" { "HK_WALK_STRIKE_LEFT" } else { "HK_WALK_STRIKE_RIGHT" };
    // A tick of the pad turns him to face the foe, and none at all when he already does (a ledge a unit
    // wide has no room for a step); the swing is then the button alone, so the wait for the nail to
    // start (a Knight in recoil cannot swing for a few ticks) never walks him anywhere.
    let turn = if side == "left" { "HK_WALK_TURN_LEFT" } else { "HK_WALK_TURN_RIGHT" };
    let _ = writeln!(out, "hold {side} until {turn} == 0 timeout 10 # {label}: face {side}");
    let _ = writeln!(out, "hold square until {word} == 0 timeout 90 # {label}: strike {side}");
    let _ = writeln!(out, "wait until HK_NAIL_BUSY == 0 timeout 90");
}

fn strike_both(out: &mut String, label: &str) {
    // Overhead first: a hit above gives no sideways recoil, which on a small platform is the difference.
    let _ = writeln!(out, "hold up+square until HK_WALK_STRIKE_UP == 0 timeout 90 # {label}: strike up");
    let _ = writeln!(out, "wait until HK_NAIL_BUSY == 0 timeout 90");
    strike_one(out, label, "right");
    strike_one(out, label, "left");
}

/// Jump at a foe that hangs above the swing's reach (a Vengefly over a ledge): hold jump until the guest's
/// `HK_WALK_JUMPSTRIKE` is 0 (the top of the rise, or the foe gone; no time at all when no foe is above), swing
/// there, and wait for the ground.
fn snipe(out: &mut String, label: &str) {
    let _ = writeln!(out, "hold cross until HK_WALK_JUMPSTRIKE == 0 timeout 60 # {label}: jump at a foe above");
    strike_both(out, label);
    let _ = writeln!(out, "wait until HK_WALK_READY == 1 timeout 300 # {label}: back on the ground");
}

/// Focus while masks are missing and SOUL pays for a heal and no foe is close: the guest publishes
/// `HK_WALK_FOCUS` as zero otherwise, so the step takes no time when Focus is not wanted.
fn heal(out: &mut String, label: &str) {
    let _ = writeln!(out, "hold circle until HK_WALK_FOCUS == 0 timeout 240 # {label}: focus");
}

/// Rewrite the walk macros into base directives. The compiler keeps the Knight's expected
/// position (`at` sets it, every `walk` and `hop` moves it), so a goal names where to go and
/// the direction to press follows from where the last goal left him:
///
/// ```text
/// at    X Y                  he stands here (the first goal after a load); waits for his control first
/// walk  X [tol T]            walk to X from either side, to within T (0.1)
/// drop  X Y                  walk to x X and fall off the edge to the ground at knight height Y (a jump pressed in
///                            mid-air does nothing, so a leap after a ledge starts from a `drop`). Every landing
///                            (`hop`, `leap`, `drop`) waits for `HK_WALK_READY`: on the ground, no hard-landing lock
/// hop   X Y [lead F] [apex A]  jump to a platform at x X, knight height Y: across F (0.75) of the way while
///                            rising, up until Y clears (or knight height A, for a gap),
///                            steer to X, land
/// leap  TX left|right N X Y [cross C]  walk to take-off x TX, jump with the direction held for N ticks in all
///                            (24, or C, of them with the jump), land at x X, knight height Y
/// bash  [N]                  N rounds (6) of leaning on whatever is ahead and swinging at it: a breakable wall
/// strike                     swing at a foe in reach on either side (nothing happens with none)
/// snipe                      jump at a foe above the swing's reach and swing at the top (a ledge with room under it; not part of `guard`)
/// heal                       Focus while masks are missing and SOUL allows and no foe is close
/// guard [N [X]]             N rounds of strike and heal where he stands (a small platform)
/// fight [N] [noheal]        N rounds of: close on the nearest foe on the right until a swing reaches
///                            it, strike, the same on the left, heal
/// ```
///
/// A `hop` that cannot rise as far as its platform times out in the rise, naming the height.
pub fn macros(text: &str) -> Result<String> {
    let mut out = String::new();
    let (mut at_x, mut at_y): (Option<f64>, Option<f64>) = (None, None);
    for (index, line) in text.lines().enumerate() {
        let located = |e: String| format!("line {}: {e}", index + 1);
        let (code, comment) = match line.split_once('#') {
            Some((code, comment)) => (code.trim(), comment.trim()),
            None => (line.trim(), ""),
        };
        let tokens: Vec<&str> = code.split_whitespace().collect();
        let label = if comment.is_empty() { code } else { comment };
        let number = |token: Option<&&str>| -> Result<f64> {
            token
                .and_then(|t| t.parse::<f64>().ok())
                .ok_or_else(|| located(format!("`{code}` needs a number")))
        };
        let (right, left) = ("right", "left");
        // Both ways round: a hold whose condition already holds ends at once, so whichever side of
        // `to` the Knight stands on, one of the two walks him in and the other does nothing. That
        // keeps a goal true after a fight has moved him, which a direction chosen when the script
        // was compiled cannot.
        let walk_to = |out: &mut String, from: f64, to: f64, tol: f64, label: &str| {
            let timeout = 300 + (40.0 * (to - from).abs().max(8.0)) as u64;
            let _ = writeln!(
                out,
                "hold right until x ge {} timeout {timeout} # {label}",
                units(to - tol)
            );
            let _ = writeln!(
                out,
                "hold left until x le {} timeout {timeout} # {label}",
                units(to + tol)
            );
        };
        match tokens.as_slice() {
            ["at", x, y] => {
                at_x = Some(number(Some(x))?);
                at_y = Some(number(Some(y))?);
                // A gate entry owns the pad until it has run its settle, its fade and its walk: nothing
                // pressed before then takes, so the first goal in a room waits for the Knight's control.
                let _ = writeln!(out, "wait until HK_WALK_READY == 1 timeout 900 # {label}: in control");
            }
            ["walk", x, rest @ ..] => {
                let to = number(Some(x))?;
                let tol = match rest {
                    ["tol", t] => number(Some(t))?,
                    [] => 0.1,
                    _ => return Err(located("walk X [tol T]".into())),
                };
                let from = at_x.unwrap_or(to);
                walk_to(&mut out, from, to, tol, label);
                at_x = Some(to);
            }
            ["drop", x, y] => {
                let (to_x, to_y) = (number(Some(x))?, number(Some(y))?);
                let from = at_x.unwrap_or(to_x);
                walk_to(&mut out, from, to_x, 0.1, label);
                let _ = writeln!(
                    out,
                    "wait until y between {} {} timeout 300 # {label}: down",
                    units(to_y - 0.15),
                    units(to_y + 0.15)
                );
                let _ = writeln!(out, "wait 6");
                let _ = writeln!(out, "wait until HK_WALK_READY == 1 timeout 300 # {label}: on the ground");
                at_x = Some(to_x);
                at_y = Some(to_y);
            }
            ["hop", x, y, rest @ ..] => {
                let (to_x, to_y) = (number(Some(x))?, number(Some(y))?);
                let (mut lead, mut apex) = (0.75, None);
                for pair in rest.chunks(2) {
                    match pair {
                        ["lead", f] => lead = number(Some(f))?,
                        ["apex", a] => apex = Some(number(Some(a))?),
                        _ => return Err(located("hop X Y [lead F] [apex A]".into())),
                    }
                }
                let (from_x, from_y) = at_x.zip(at_y).ok_or_else(|| located("hop needs an `at X Y` first".into()))?;
                // The platform's own x: a hop lands where the knight is let go, not short of it.
                let (dir, op, target) =
                    if to_x >= from_x { (right, "ge", to_x) } else { (left, "le", to_x) };
                if apex.is_some() || to_y - from_y > 0.3 {
                    let top = apex.unwrap_or(to_y + 0.35);
                    if lead > 0.0 {
                        // Most of the way across while the jump starts, then straight up until the
                        // feet clear the platform: pressing across all the way up carries the Knight
                        // past it, and letting go early leaves no time to cross before the fall.
                        let half = from_x + (to_x - from_x) * lead;
                        let _ = writeln!(
                            out,
                            "hold {dir}+cross until x {op} {} timeout 60 # {label}: jump across",
                            units(half)
                        );
                        let _ = writeln!(
                            out,
                            "hold cross until y ge {} timeout 120 # {label}: rise",
                            units(top)
                        );
                    } else {
                        // `lead 0`: a wall stands between, so press into it and slide up its face.
                        let _ = writeln!(
                            out,
                            "hold {dir}+cross until y ge {} timeout 120 # {label}: rise along the wall",
                            units(top)
                        );
                    }
                }
                let _ = writeln!(
                    out,
                    "hold {dir} until x {op} {} timeout 150 # {label}: steer",
                    units(target)
                );
                let _ = writeln!(
                    out,
                    "wait until y between {} {} timeout 180 # {label}: land",
                    units(to_y - 0.15),
                    units(to_y + 0.15)
                );
                let _ = writeln!(out, "wait 6");
                let _ = writeln!(out, "wait until HK_WALK_READY == 1 timeout 300 # {label}: on the ground");
                at_x = Some(to_x);
                at_y = Some(to_y);
            }
            ["leap", tx, dir, ticks, x, y, rest @ ..] => {
                let cross = match rest {
                    [] => 24,
                    ["cross", c] => number(Some(c))? as u64,
                    _ => return Err(located("leap TX left|right N X Y [cross C]".into())),
                };
                let (take_x, to_x, to_y) = (number(Some(tx))?, number(Some(x))?, number(Some(y))?);
                let held = match *dir {
                    "left" | "right" => *dir,
                    _ => return Err(located("leap TX left|right TICKS X Y".into())),
                };
                let from = at_x.ok_or_else(|| located("leap needs an `at X Y` first".into()))?;
                let mut ticks = number(Some(ticks))? as u64;
                // Back on the platform's own mark first: a fight may have moved him.
                walk_to(&mut out, from, from, 0.15, &format!("{label}: back on the mark"));
                if (take_x - from).abs() > 0.05 {
                    // The jump goes out a tick after the walk ends and a ledge has no room for the
                    // difference: let go 0.3 units early and cross that much longer in the air.
                    let early = take_x + if held == "left" { LEAP_EARLY } else { -LEAP_EARLY };
                    ticks += (LEAP_EARLY / WALK_PER_TICK).round() as u64;
                    let op = if held == "left" { "le" } else { "ge" };
                    let _ = writeln!(
                        out,
                        "hold {held} until x {op} {} timeout 200 # {label}: to the take-off",
                        units(early)
                    );
                }
                // The jump itself, the pad as a player gives it: jump held with the direction for
                // a rise, the direction alone for the rest of the way across. What closes the loop
                // is where it starts (take-off x) and where it must end (the landing).
                let _ = writeln!(out, "hold {held}+cross for {cross} # {label}: jump");
                if ticks > cross {
                    // Across to the landing's own x, not for a count of ticks: a take-off a few
                    // tenths off, or a hit on the way, then still ends on the platform.
                    let (op, stop) =
                        if held == "left" { ("le", to_x + 0.25) } else { ("ge", to_x - 0.25) };
                    let _ = writeln!(
                        out,
                        "hold {held} until x {op} {} timeout 200 # {label}: across",
                        units(stop)
                    );
                }
                let _ = writeln!(
                    out,
                    "wait until y between {} {} timeout 180 # {label}: land",
                    units(to_y - 0.15),
                    units(to_y + 0.15)
                );
                let _ = writeln!(out, "wait 6");
                let _ = writeln!(out, "wait until HK_WALK_READY == 1 timeout 300 # {label}: on the ground");
                at_x = Some(to_x);
                at_y = Some(to_y);
            }
            ["bash", rest @ ..] => {
                let swings = if rest.is_empty() { 6 } else { number(rest.first())? as u64 };
                // Pressed against the wall before each swing: a hit's recoil pushes him back out of reach.
                for _ in 0..swings {
                    let _ = writeln!(out, "hold right for 4 # {label}: lean on it");
                    let _ = writeln!(out, "press square for 2 rest 22");
                }
            }
            ["strike"] => strike_both(&mut out, label),
            ["snipe"] => snipe(&mut out, label),
            ["heal"] => heal(&mut out, label),
            ["guard", rest @ ..] => {
                let rounds = if rest.is_empty() { 2 } else { number(rest.first())? as u64 };
                // `guard N X`: back to x X after every round (a hit pushes him, a tap of the pad
                // moves him), so a small platform stays under his feet.
                let mark = match rest.get(1) {
                    Some(x) => Some(number(Some(x))?),
                    None => None,
                };
                // Stand where he is, strike whatever comes into reach on either side, heal if he
                // may: nothing here moves him anywhere that matters, so a small platform stays
                // under his feet.
                for _ in 0..rounds {
                    strike_both(&mut out, label);
                    heal(&mut out, label);
                    // Time to pass: a foe that is not here yet arrives while the rounds run.
                    let _ = writeln!(out, "wait 4");
                    if let Some(x) = mark {
                        walk_to(&mut out, x, x, 0.4, &format!("{label}: back on the mark"));
                    }
                }
            }
            ["fight", rest @ ..] => {
                let rounds = if rest.is_empty() { 3 } else { number(rest.first())? as u64 };
                // `fight N noheal`: leave SOUL alone (a proof that counts it).
                let heals = !rest.contains(&"noheal");
                for round in 1..=rounds {
                    let _ = writeln!(out, "hold right until HK_FOE_GAP_RIGHT == 0 timeout 300 # {label}: round {round}, close on the right");
                    strike_one(&mut out, label, "right");
                    let _ = writeln!(out, "hold left until HK_FOE_GAP_LEFT == 0 timeout 300 # {label}: round {round}, close on the left");
                    strike_one(&mut out, label, "left");
                    if heals {
                        heal(&mut out, label);
                    }
                }
            }
            _ => {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    Ok(out)
}

/// The script at `path` with every `include FILE` line (relative to the including file) replaced by
/// that file's lines, so a prologue is written once.
pub fn expand(path: &Path) -> Result<String> {
    expand_depth(path, 0)
}

fn expand_depth(path: &Path, depth: u32) -> Result<String> {
    if depth > 8 {
        return Err(format!("{}: includes nest too deep", path.display()));
    }
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut out = String::new();
    for line in text.lines() {
        match line.split_whitespace().collect::<Vec<_>>().as_slice() {
            ["include", file] => {
                let base = path.parent().unwrap_or(Path::new("."));
                out.push_str(&expand_depth(&base.join(file), depth + 1)?);
            }
            _ => {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    Ok(out)
}

fn options_of(tokens: &[&str], allowed: &[(&str, &str)]) -> Result<BTreeMap<String, u64>> {
    let found = options(tokens)?;
    for key in found.keys() {
        if !allowed.iter().any(|(k, _)| k == key) {
            return Err(format!("unexpected `{key}`"));
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compile_text(text: &str) -> Result<String> {
        compile(text, &Setup { disc: Path::new("/d.cue"), symbols: Path::new("/g.map") })
    }

    #[test]
    fn a_walk_toward_x_holds_until_the_word_crosses_in_q16() {
        let toml = compile_text("name t\nhold right until x ge 12.5 # walk\n").unwrap();
        assert!(toml.contains("press = [\"right\"]"));
        assert!(toml.contains("[step.hold_until]"));
        assert!(toml.contains("sym = \"HK_PLAYER_X\""));
        assert!(toml.contains("signed = true"));
        assert!(toml.contains("op = \"ge\"\nvalue = 819200"));
        assert!(toml.contains("label = \"walk\""));
    }

    #[test]
    fn between_and_symbols_take_integers() {
        let toml = compile_text("name t\nwait until HK_REGION_ID == 79 timeout 900\n").unwrap();
        assert!(toml.contains("timeout = 900"));
        assert!(toml.contains("[step.wait_until]"));
        assert!(toml.contains("op = \"eq\"\nvalue = 79"));
        let toml = compile_text("name t\nwait until y between 10 11.5\n").unwrap();
        assert!(toml.contains("value = 655360\nvalue_hi = 753664"));
    }

    #[test]
    fn taps_swings_and_probes_become_steps() {
        let toml =
            compile_text("name t\nrepeat 3 press square for 2 rest 22\nprobe p x y HK_HEALTH\n").unwrap();
        assert!(toml.contains("hold = 2\nwait = 22\nrepeat = 3"));
        assert_eq!(toml.matches("[[step.assert]]").count(), 3);
        assert!(toml.contains("checkpoint = \"p\""));
    }

    #[test]
    fn checks_split_on_semicolons_and_aliases_resolve() {
        let toml = compile_text("name t\nsym hp HK_HEALTH\ncheck end hp ge 3 ; x lt 40\n").unwrap();
        assert_eq!(toml.matches("[[step.assert]]").count(), 2);
        assert!(toml.contains("sym = \"HK_HEALTH\""));
    }

    #[test]
    fn a_drop_waits_for_the_ground_before_the_next_goal() {
        let toml = compile_text("name t\nat 10 12.39\ndrop 12 9.39\nleap 12 right 60 20 9.39\n").unwrap();
        // x ge 11.9, then the landing band 9.24 to 9.54 (in Q16), then the leap's own take-off press.
        let land = toml.find("value = 605553\nvalue_hi = 625213").expect("a landing band");
        let jump = toml.find("press = [\"right\", \"cross\"]").expect("a jump");
        assert!(land < jump, "{toml}");
    }

    #[test]
    fn macros_walk_from_both_sides_and_react_for_free() {
        let toml = compile_text(
            "name t\nat 10 5.39\nwalk 20\nhop 24 9.98\nleap 24 left 36 19 14.35\nbash 3\nguard 1\nfight 1\n",
        )
        .unwrap();
        // walk 20: right until 19.9, left until 20.1, whichever side he is on.
        assert!(toml.contains("op = \"ge\"\nvalue = 1304166"), "{toml}");
        assert!(toml.contains("op = \"le\"\nvalue = 1317274"), "{toml}");
        assert!(toml.contains("press = [\"right\", \"cross\"]"));
        assert!(toml.contains("press = [\"left\", \"cross\"]"));
        assert!(toml.contains("hold = 2\nwait = 22"));
        assert!(toml.contains("press = [\"square\"]"));
        assert!(toml.contains("sym = \"HK_WALK_STRIKE_RIGHT\""));
        assert!(toml.contains("sym = \"HK_NAIL_BUSY\""));
        assert!(toml.contains("sym = \"HK_WALK_FOCUS\""));
        assert!(toml.contains("HK_FOE_GAP_RIGHT") && toml.contains("HK_FOE_GAP_LEFT"));
        assert!(compile_text("name t\nhop 3 3\n").unwrap_err().contains("at X Y"));
    }

    #[test]
    fn mistakes_are_named() {
        assert!(compile_text("name t\nhold sideways for 3\n").unwrap_err().contains("sideways"));
        assert!(compile_text("name t\nwait until x ~ 3\n").unwrap_err().contains("operator"));
        assert!(compile_text("hold right for 3\n").unwrap_err().contains("name"));
        assert!(compile_text("name t\njump 3\n").unwrap_err().contains("line 2"));
    }
}
