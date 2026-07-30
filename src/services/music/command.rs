use crate::config::MusicConfig;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MusicCommand {
    Play(String),
    Singer(String),
    Playlist(String),
    Random(usize),
    Pause,
    Resume,
    Next,
    Previous,
    Stop,
    Shuffle,
    RepeatOne,
    RepeatAll,
}

#[derive(Clone)]
pub struct MusicCommandParser {
    config: MusicConfig,
}

impl MusicCommandParser {
    pub fn new(config: MusicConfig) -> Self {
        Self { config }
    }

    pub fn parse(&self, input: &str, session_active: bool) -> Option<MusicCommand> {
        if !self.config.enabled {
            return None;
        }
        let mut text = normalize(input);
        if text.is_empty() {
            return None;
        }

        let had_required = strip_longest_prefix(&mut text, &self.config.commands.required_prefixes);
        if !self.config.commands.required_prefixes.is_empty() && !had_required {
            return None;
        }
        let explicit = strip_longest_prefix(&mut text, &self.config.commands.explicit_prefixes);
        let controls_allowed = session_active || explicit || had_required;

        if controls_allowed {
            if exact(&text, &self.config.commands.resume_words) {
                return Some(MusicCommand::Resume);
            }
            if exact(&text, &self.config.commands.pause_words) {
                return Some(MusicCommand::Pause);
            }
            if exact(&text, &self.config.commands.next_words) {
                return Some(MusicCommand::Next);
            }
            if exact(&text, &self.config.commands.previous_words) {
                return Some(MusicCommand::Previous);
            }
            if exact(&text, &self.config.commands.stop_words) {
                return Some(MusicCommand::Stop);
            }
            if exact(&text, &self.config.commands.repeat_one_words) {
                return Some(MusicCommand::RepeatOne);
            }
            if exact(&text, &self.config.commands.repeat_all_words) {
                return Some(MusicCommand::RepeatAll);
            }
            if exact(&text, &self.config.commands.shuffle_words) {
                return Some(if session_active {
                    MusicCommand::Shuffle
                } else {
                    MusicCommand::Random(self.config.queue.autoplay_count.max(1))
                });
            }
        }

        if exact(&text, &self.config.commands.shuffle_words) {
            return Some(MusicCommand::Random(
                self.config.queue.autoplay_count.max(1),
            ));
        }

        let has_play_word = strip_longest_prefix(&mut text, &self.config.commands.play_words);
        if self.config.route_mode == "prefix_only" && !explicit && !had_required {
            return None;
        }
        if !explicit && !had_required && !has_play_word {
            return None;
        }
        if text.is_empty() {
            return None;
        }

        if strip_longest_prefix(&mut text, &self.config.commands.playlist_words) {
            return non_empty(text).map(MusicCommand::Playlist);
        }
        if strip_longest_prefix(&mut text, &self.config.commands.singer_words) {
            return non_empty(text).map(MusicCommand::Singer);
        }
        for suffix in &self.config.commands.singer_words {
            if let Some(name) = text.strip_suffix(suffix) {
                if let Some(name) = non_empty(name.to_string()) {
                    return Some(MusicCommand::Singer(name));
                }
            }
        }
        Some(MusicCommand::Play(text))
    }
}

fn normalize(text: &str) -> String {
    text.trim()
        .chars()
        .filter(|c| !c.is_whitespace() && !"，。！？、,.!?".contains(*c))
        .collect()
}

fn exact(text: &str, words: &[String]) -> bool {
    words.iter().any(|word| text == normalize(word))
}

fn strip_longest_prefix(text: &mut String, words: &[String]) -> bool {
    let prefix = words
        .iter()
        .map(|word| normalize(word))
        .filter(|word| !word.is_empty() && text.starts_with(word))
        .max_by_key(String::len);
    if let Some(prefix) = prefix {
        *text = text[prefix.len()..].to_string();
        true
    } else {
        false
    }
}

fn non_empty(text: String) -> Option<String> {
    let text = text.trim().to_string();
    (!text.is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parser() -> MusicCommandParser {
        let mut config = MusicConfig::default();
        config.enabled = true;
        MusicCommandParser::new(config)
    }

    #[test]
    fn parses_song_singer_playlist_and_controls() {
        let parser = parser();
        assert_eq!(
            parser.parse("播放周杰伦的晴天", false),
            Some(MusicCommand::Play("周杰伦的晴天".into()))
        );
        assert_eq!(
            parser.parse("播放周杰伦的歌", false),
            Some(MusicCommand::Singer("周杰伦".into()))
        );
        assert_eq!(
            parser.parse("播放歌单华语经典", false),
            Some(MusicCommand::Playlist("华语经典".into()))
        );
        assert_eq!(parser.parse("下一首", true), Some(MusicCommand::Next));
        assert_eq!(parser.parse("下一首", false), None);
    }

    #[test]
    fn prefix_only_does_not_capture_normal_music() {
        let mut config = MusicConfig::default();
        config.enabled = true;
        config.route_mode = "prefix_only".into();
        let parser = MusicCommandParser::new(config);
        assert_eq!(parser.parse("播放晴天", false), None);
        assert_eq!(
            parser.parse("本地音乐播放晴天", false),
            Some(MusicCommand::Play("晴天".into()))
        );
    }
}
