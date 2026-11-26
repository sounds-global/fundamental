use std::time::Duration;

/// A live run's settings.
#[derive(Debug, Clone)]
pub struct LiveArgs {
    /// How long each load step is measured.
    pub each: Duration,
    /// The concurrency of each load step.
    pub concurrency: Vec<usize>,
    /// How long a soak runs.
    pub duration: Duration,
    /// How many numbered writes each fault run makes.
    pub writes: u64,
}

impl LiveArgs {
    /// The process's arguments over the defaults.
    ///
    /// # Errors
    ///
    /// A word that is not a setting, or a setting's value that does not read.
    pub fn from_env() -> Result<Self, String> {
        Self::parse(std::env::args().skip(1))
    }

    /// `words` over the defaults: 20 s a step at 1, 8, 32 and 128, a ten-minute soak, 300 writes.
    ///
    /// # Errors
    ///
    /// A word that is not a setting, or a setting's value that does not read.
    pub fn parse(words: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut args = Self {
            each: Duration::from_secs(20),
            concurrency: vec![1, 8, 32, 128],
            duration: Duration::from_secs(600),
            writes: 300,
        };
        let mut words = words.into_iter();
        while let Some(word) = words.next() {
            let mut value = || words.next().ok_or_else(|| format!("{word} needs a value"));
            match word.as_str() {
                "--each" => args.each = duration(&value()?)?,
                "--duration" => args.duration = duration(&value()?)?,
                "--concurrency" => args.concurrency = concurrency(&value()?)?,
                "--writes" => {
                    let writes = value()?;
                    args.writes = writes
                        .parse()
                        .map_err(|_| format!("{writes} is not a count"))?;
                }
                "--bench" => {}
                other => return Err(format!("{other} is not a setting of this run")),
            }
        }
        Ok(args)
    }
}

/// `90s`, `10m`, `1h` or a bare number of seconds.
fn duration(text: &str) -> Result<Duration, String> {
    let (number, unit) = text.trim().split_at(
        text.trim()
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(text.trim().len()),
    );
    let number: u64 = number
        .parse()
        .map_err(|_| format!("{text} is not a duration"))?;
    let seconds = match unit {
        "" | "s" => number,
        "m" => number * 60,
        "h" => number * 3_600,
        _ => return Err(format!("{text} is not a duration: use s, m or h")),
    };
    Ok(Duration::from_secs(seconds))
}

/// `1,8,32` as the concurrency of each step.
fn concurrency(text: &str) -> Result<Vec<usize>, String> {
    text.split(',').map(step).collect()
}

fn step(n: &str) -> Result<usize, String> {
    n.trim()
        .parse()
        .map_err(|_| format!("{n} is not a concurrency"))
}
