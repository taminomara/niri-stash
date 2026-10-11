//! The hotkey-client ⇄ daemon line protocol. Pure.
//!
//! `request = ("stash" / "toggle" / "unstash") LF`
//! `reply   = ("ok" / "err" SP code SP message) LF`

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HotkeyRequest {
    /// Stash every floating window on the focused workspace that is not stashed yet.
    Stash,
    /// Stash as above, or restore every stashed window when nothing is left to stash.
    Toggle,
    /// Restore every stashed window on the focused workspace.
    Unstash,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReplyErrorCode {
    NiriUnavailable,
    StateUnwritable,
    UnknownCommand,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum HotkeyReply {
    Ok,
    Err {
        code: ReplyErrorCode,
        message: String,
    },
}

impl ReplyErrorCode {
    /// The code as it appears on the wire.
    fn wire_name(self) -> &'static str {
        match self {
            ReplyErrorCode::NiriUnavailable => "niri-unavailable",
            ReplyErrorCode::StateUnwritable => "state-unwritable",
            ReplyErrorCode::UnknownCommand => "unknown-command",
        }
    }

    /// The code for a wire token, or `None` for anything that is not one of the three codes.
    fn from_wire_name(token: &str) -> Option<Self> {
        match token {
            "niri-unavailable" => Some(ReplyErrorCode::NiriUnavailable),
            "state-unwritable" => Some(ReplyErrorCode::StateUnwritable),
            "unknown-command" => Some(ReplyErrorCode::UnknownCommand),
            _ => None,
        }
    }
}

/// The request as the client sends it, including the trailing LF.
pub(crate) fn format_request_line(req: HotkeyRequest) -> String {
    let verb = match req {
        HotkeyRequest::Stash => "stash",
        HotkeyRequest::Toggle => "toggle",
        HotkeyRequest::Unstash => "unstash",
    };
    format!("{verb}\n")
}

/// Parse one request line (with or without its trailing LF). Anything else is `None`.
pub(crate) fn parse_request_line(line: &str) -> Option<HotkeyRequest> {
    match strip_line_end(line) {
        "stash" => Some(HotkeyRequest::Stash),
        "toggle" => Some(HotkeyRequest::Toggle),
        "unstash" => Some(HotkeyRequest::Unstash),
        _ => None,
    }
}

/// The reply as the daemon sends it, including the trailing LF. The message must contain no LF;
/// any LF in it is replaced by a space.
pub(crate) fn format_reply_line(reply: &HotkeyReply) -> String {
    match reply {
        HotkeyReply::Ok => "ok\n".to_string(),
        HotkeyReply::Err { code, message } => {
            let message = message.replace(['\n', '\r'], " ");
            format!("err {} {message}\n", code.wire_name())
        }
    }
}

/// Parse one reply line (with or without its trailing LF). Anything else is `None`.
pub(crate) fn parse_reply_line(line: &str) -> Option<HotkeyReply> {
    let body = strip_line_end(line);
    if body == "ok" {
        return Some(HotkeyReply::Ok);
    }
    let rest = body.strip_prefix("err ")?;
    let (code, message) = rest.split_once(' ')?;
    let code = ReplyErrorCode::from_wire_name(code)?;
    Some(HotkeyReply::Err {
        code,
        message: message.to_string(),
    })
}

/// Remove one trailing LF (and an optional CR before it); nothing else is trimmed.
fn strip_line_end(line: &str) -> &str {
    match line.strip_suffix('\n') {
        Some(body) => body.strip_suffix('\r').unwrap_or(body),
        None => line,
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_three_requests_with_and_without_newline() {
        assert_eq!(parse_request_line("stash"), Some(HotkeyRequest::Stash));
        assert_eq!(parse_request_line("stash\n"), Some(HotkeyRequest::Stash));
        assert_eq!(parse_request_line("toggle"), Some(HotkeyRequest::Toggle));
        assert_eq!(parse_request_line("toggle\n"), Some(HotkeyRequest::Toggle));
        assert_eq!(parse_request_line("unstash"), Some(HotkeyRequest::Unstash));
        assert_eq!(
            parse_request_line("unstash\n"),
            Some(HotkeyRequest::Unstash)
        );
        assert_eq!(
            parse_request_line("toggle\r\n"),
            Some(HotkeyRequest::Toggle)
        );
    }

    #[test]
    fn rejects_any_other_request() {
        for bad in [
            "",
            "\n",
            "Toggle",
            "toggle now",
            " toggle",
            "toggle\n\n",
            "toggle\r",
            "status",
        ] {
            assert_eq!(parse_request_line(bad), None, "input {bad:?}");
        }
    }

    #[test]
    fn request_lines_end_with_a_newline() {
        assert_eq!(format_request_line(HotkeyRequest::Stash), "stash\n");
        assert_eq!(format_request_line(HotkeyRequest::Toggle), "toggle\n");
        assert_eq!(format_request_line(HotkeyRequest::Unstash), "unstash\n");
    }

    #[test]
    fn ok_reply_line_is_ok_newline() {
        assert_eq!(format_reply_line(&HotkeyReply::Ok), "ok\n");
    }

    #[test]
    fn err_reply_line_carries_code_and_message_without_embedded_newlines() {
        let plain = HotkeyReply::Err {
            code: ReplyErrorCode::StateUnwritable,
            message: "rename /run/user/1000/niri-stash/state.json: Read-only file system".into(),
        };
        assert_eq!(
            format_reply_line(&plain),
            "err state-unwritable rename /run/user/1000/niri-stash/state.json: Read-only file system\n"
        );
        for (code, wire) in [
            (ReplyErrorCode::NiriUnavailable, "niri-unavailable"),
            (ReplyErrorCode::StateUnwritable, "state-unwritable"),
            (ReplyErrorCode::UnknownCommand, "unknown-command"),
        ] {
            let reply = HotkeyReply::Err {
                code,
                message: "m".into(),
            };
            assert_eq!(format_reply_line(&reply), format!("err {wire} m\n"));
        }
        let multiline = HotkeyReply::Err {
            code: ReplyErrorCode::NiriUnavailable,
            message: "a\nb\r\nc".into(),
        };
        assert_eq!(
            format_reply_line(&multiline),
            "err niri-unavailable a b  c\n"
        );
    }

    #[test]
    fn parses_an_ok_reply() {
        assert_eq!(parse_reply_line("ok"), Some(HotkeyReply::Ok));
        assert_eq!(parse_reply_line("ok\n"), Some(HotkeyReply::Ok));
    }

    #[test]
    fn parses_an_err_reply_with_a_multi_word_message() {
        assert_eq!(
            parse_reply_line("err state-unwritable rename state.json: Read-only file system\n"),
            Some(HotkeyReply::Err {
                code: ReplyErrorCode::StateUnwritable,
                message: "rename state.json: Read-only file system".into(),
            })
        );
        assert_eq!(
            parse_reply_line("err unknown-command "),
            Some(HotkeyReply::Err {
                code: ReplyErrorCode::UnknownCommand,
                message: String::new()
            })
        );
        assert_eq!(
            parse_reply_line("err niri-unavailable  two spaces"),
            Some(HotkeyReply::Err {
                code: ReplyErrorCode::NiriUnavailable,
                message: " two spaces".into()
            })
        );
        assert_eq!(
            parse_reply_line("err unknown-command x\n"),
            Some(HotkeyReply::Err {
                code: ReplyErrorCode::UnknownCommand,
                message: "x".into()
            })
        );
    }

    #[test]
    fn rejects_an_unknown_code_or_malformed_reply() {
        for bad in [
            "err disk-full something",
            "err  unknown-command x",
            "err unknown-command",
            "err busy-timeout x",
            "err",
            "",
            "OK",
            "ok ",
            "ok\n\n",
            "fail unknown-command x",
        ] {
            assert_eq!(parse_reply_line(bad), None, "input {bad:?}");
        }
    }
}
