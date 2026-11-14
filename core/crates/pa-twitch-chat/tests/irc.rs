// SPDX-License-Identifier: MPL-2.0
//! IRCv3 parsing and normalisation on a recorded (hand-written, made-up
//! names) anonymous session: `tests/fixtures/irc/session.txt`.

use std::time::Duration;

use pa_twitch_chat::irc::{IrcInput, interpret, parse_frame, parse_line, unescape_tag};
use pa_twitch_chat::{
    Badge, ChannelRef, ChatEvent, ChatMessage, ClearAction, EmoteRange, NoticeKind, RoomModes,
    SourceStatus,
};

const SESSION: &str = include_str!("fixtures/irc/session.txt");
const NICK: &str = "justinfan48213";

fn channel() -> ChannelRef {
    ChannelRef {
        id: "100000001".to_owned(),
        login: "kinolune_demo".to_owned(),
    }
}

fn inputs() -> Vec<IrcInput> {
    parse_frame(SESSION)
        .iter()
        .map(|m| interpret(m, NICK))
        .collect()
}

fn message(input: &IrcInput) -> &ChatMessage {
    match input {
        IrcInput::Event(ChatEvent::Message(m)) => m,
        other => panic!("expected a message, got {other:?}"),
    }
}

fn emote_text(m: &ChatMessage, e: &EmoteRange) -> String {
    m.text.chars().skip(e.start).take(e.end - e.start).collect()
}

#[test]
fn parses_every_line_of_the_session() {
    assert_eq!(parse_frame(SESSION).len(), SESSION.lines().count());
    // CRLF framing as Twitch sends it, several lines per frame.
    let crlf = SESSION.replace('\n', "\r\n");
    assert_eq!(parse_frame(&crlf), parse_frame(SESSION));
}

#[test]
fn line_grammar() {
    let m = parse_line("@a=1;b;c=x\\sy :nick!nick@host PRIVMSG #chan :hello :world").unwrap();
    assert_eq!(m.tags["a"], "1");
    assert_eq!(m.tags["b"], "");
    assert_eq!(m.tags["c"], "x y");
    assert_eq!(m.nick(), Some("nick"));
    assert_eq!(m.command, "PRIVMSG");
    assert_eq!(m.params, ["#chan", "hello :world"]);

    let ping = parse_line("PING :tmi.twitch.tv").unwrap();
    assert_eq!(ping.prefix, None);
    assert_eq!(ping.params, ["tmi.twitch.tv"]);

    assert!(parse_line("").is_none());
    assert!(parse_line("@only-tags").is_none());
}

#[test]
fn tag_unescaping() {
    assert_eq!(unescape_tag(r"a\sb\:c\\d\re\nf"), "a b;c\\d\re\nf");
    // Unknown escape keeps the character, trailing backslash is dropped.
    assert_eq!(unescape_tag(r"x\qy\"), "xqy");
}

#[test]
fn control_lines() {
    let inputs = inputs();
    assert_eq!(inputs[0], IrcInput::Ignore); // CAP ACK
    assert_eq!(inputs[1], IrcInput::Welcome); // 001
    assert_eq!(inputs[2], IrcInput::Ignore); // 375
    assert_eq!(
        inputs[3],
        IrcInput::Event(ChatEvent::Status(SourceStatus::Joined(ChannelRef {
            id: String::new(),
            login: "kinolune_demo".to_owned(),
        })))
    );
    assert_eq!(
        inputs.last().unwrap(),
        &IrcInput::Ping("tmi.twitch.tv".to_owned())
    );
    let reconnect = parse_line(":tmi.twitch.tv RECONNECT").unwrap();
    assert_eq!(interpret(&reconnect, NICK), IrcInput::Reconnect);
    // Someone else's JOIN is not our join.
    let other =
        parse_line(":pixel_otter!pixel_otter@pixel_otter.tmi.twitch.tv JOIN #kinolune_demo")
            .unwrap();
    assert_eq!(interpret(&other, NICK), IrcInput::Ignore);
}

#[test]
fn privmsg_with_badges_colour_and_emotes() {
    let inputs = inputs();
    let m = message(&inputs[5]);
    assert_eq!(m.id, "7c1e5f0a-0001-4a5b-9c1d-000000000001");
    assert_eq!(m.ts_ms, 1_794_650_000_123);
    assert_eq!(m.channel, channel());
    assert_eq!(m.user.id, "200000001");
    assert_eq!(m.user.login, "pixel_otter");
    assert_eq!(m.user.display_name, "Pixel_Otter");
    assert_eq!(m.user.color.as_deref(), Some("#1E90FF"));
    assert_eq!(
        m.user.badges,
        vec![
            Badge {
                set_id: "subscriber".to_owned(),
                version: "12".to_owned(),
                info: Some("14".to_owned()),
            },
            Badge {
                set_id: "premium".to_owned(),
                version: "1".to_owned(),
                info: None,
            },
        ]
    );
    assert_eq!(m.text, "Kappa Keepo Kappa");
    let ids: Vec<_> = m.emotes.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(ids, ["25", "1902", "25"], "sorted by position");
    for e in &m.emotes {
        let code = emote_text(m, e);
        assert_eq!(code, if e.id == "25" { "Kappa" } else { "Keepo" });
    }
    assert!(!m.is_action && !m.is_first_message && !m.is_highlighted);
    assert_eq!(m.bits, None);
    assert_eq!(m.reply, None);
    assert_eq!(m.notice, None);
}

#[test]
fn emote_offsets_are_characters_not_bytes() {
    let inputs = inputs();
    let m = message(&inputs[6]);
    assert_eq!(m.text, "héllo 🎉 Kappa");
    assert_eq!(m.emotes.len(), 1);
    assert_eq!(emote_text(m, &m.emotes[0]), "Kappa");
    assert!(m.is_first_message);
    // Empty colour tag means "no colour picked".
    assert_eq!(m.user.color, None);
    assert!(m.user.badges.is_empty());
}

#[test]
fn reply_thread() {
    let inputs = inputs();
    let m = message(&inputs[7]);
    let reply = m.reply.as_ref().unwrap();
    assert_eq!(
        reply.parent_message_id,
        "7c1e5f0a-0001-4a5b-9c1d-000000000001"
    );
    assert_eq!(reply.parent_user_id, "200000001");
    assert_eq!(reply.parent_user_login, "pixel_otter");
    assert_eq!(reply.parent_display_name, "Pixel_Otter");
    assert_eq!(reply.parent_body, "Kappa Keepo Kappa");
    assert_eq!(
        reply.thread_message_id.as_deref(),
        Some("7c1e5f0a-0001-4a5b-9c1d-000000000001")
    );
    assert_eq!(reply.thread_user_login.as_deref(), Some("pixel_otter"));
    assert_eq!(m.text, "@Pixel_Otter nice one");
    assert_eq!(m.user.badges[0].set_id, "moderator");
}

#[test]
fn bits() {
    let inputs = inputs();
    let m = message(&inputs[8]);
    assert_eq!(m.bits, Some(100));
    assert_eq!(m.text, "Cheer100 great stream");
}

#[test]
fn usernotices() {
    let inputs = inputs();

    let resub = message(&inputs[9]);
    let notice = resub.notice.as_ref().unwrap();
    assert_eq!(notice.kind, NoticeKind::Resub);
    assert_eq!(
        notice.system_text,
        "QuietHeron subscribed at Tier 1. They've subscribed for 15 months!"
    );
    assert_eq!(notice.params["cumulative-months"], "15");
    assert_eq!(
        notice.params["sub-plan-name"],
        "Channel Subscription (kinolune_demo)"
    );
    assert_eq!(resub.text, "fifteen months already");
    assert_eq!(resub.user.login, "quiet_heron");

    let raid = message(&inputs[10]);
    assert_eq!(
        raid.notice.as_ref().unwrap().kind,
        NoticeKind::Raid {
            viewer_count: Some(42)
        }
    );
    assert_eq!(raid.text, "");
    assert_eq!(raid.user.login, "moss_lantern");

    let announcement = message(&inputs[11]);
    assert_eq!(
        announcement.notice.as_ref().unwrap().kind,
        NoticeKind::Announcement {
            color: Some("PRIMARY".to_owned())
        }
    );
    assert_eq!(announcement.text, "Stream starts in 5 minutes");
}

#[test]
fn moderation_events() {
    let inputs = inputs();
    assert_eq!(
        inputs[12],
        IrcInput::Event(ChatEvent::MessageDeleted {
            channel: channel(),
            message_id: "7c1e5f0a-0001-4a5b-9c1d-000000000001".to_owned(),
            user_login: Some("pixel_otter".to_owned()),
        })
    );
    assert_eq!(
        inputs[13],
        IrcInput::Event(ChatEvent::UserCleared {
            channel: channel(),
            user_id: "200000003".to_owned(),
            user_login: "moss_lantern".to_owned(),
            action: ClearAction::Timeout(Duration::from_secs(600)),
        })
    );
    assert_eq!(
        inputs[14],
        IrcInput::Event(ChatEvent::UserCleared {
            channel: channel(),
            user_id: "200000004".to_owned(),
            user_login: "spam_sparrow".to_owned(),
            action: ClearAction::Ban,
        })
    );
    assert_eq!(
        inputs[15],
        IrcInput::Event(ChatEvent::ChatCleared { channel: channel() })
    );
    assert_eq!(
        inputs[17],
        IrcInput::Event(ChatEvent::Notice {
            channel_login: Some("kinolune_demo".to_owned()),
            notice_id: Some("emote_only_on".to_owned()),
            text: "This room is now in emote-only mode.".to_owned(),
        })
    );
}

#[test]
fn room_modes_full_and_partial() {
    let inputs = inputs();
    let IrcInput::Event(ChatEvent::RoomModes { channel: c, modes }) = &inputs[4] else {
        panic!("expected ROOMSTATE");
    };
    assert_eq!(c, &channel());
    let mut state = RoomModes::default();
    state.merge(modes);
    assert_eq!(
        state,
        RoomModes {
            emote_only: Some(false),
            followers_only: Some(Some(Duration::from_secs(600))),
            unique_chat: Some(false),
            slow: Some(Some(Duration::from_secs(30))),
            subscribers_only: Some(false),
        }
    );
    let IrcInput::Event(ChatEvent::RoomModes { modes: update, .. }) = &inputs[16] else {
        panic!("expected partial ROOMSTATE");
    };
    assert_eq!(update.slow, Some(None));
    assert_eq!(update.followers_only, None);
    state.merge(update);
    assert_eq!(state.slow, Some(None), "slow mode turned off");
    assert_eq!(
        state.followers_only,
        Some(Some(Duration::from_secs(600))),
        "untouched"
    );

    let off = parse_line("@followers-only=-1;room-id=1 :tmi.twitch.tv ROOMSTATE #a").unwrap();
    let IrcInput::Event(ChatEvent::RoomModes { modes, .. }) = interpret(&off, NICK) else {
        panic!();
    };
    assert_eq!(modes.followers_only, Some(None));
}

#[test]
fn action_messages() {
    let line = "@badges=;color=#00FF7F;display-name=Pixel_Otter;emotes=25:6-10;id=act-1;room-id=100000001;tmi-sent-ts=1794650011000;user-id=200000001 :pixel_otter!pixel_otter@pixel_otter.tmi.twitch.tv PRIVMSG #kinolune_demo :\u{1}ACTION waves Kappa\u{1}";
    let input = interpret(&parse_line(line).unwrap(), NICK);
    let m = message(&input);
    assert!(m.is_action);
    assert_eq!(m.text, "waves Kappa");
    assert_eq!(emote_text(m, &m.emotes[0]), "Kappa");
}

#[test]
fn bad_emote_ranges_are_dropped() {
    let line =
        "@emotes=25:0-4,10-99/x:a-b;id=e1;room-id=1 :u!u@u.tmi.twitch.tv PRIVMSG #a :Kappa hi";
    let input = interpret(&parse_line(line).unwrap(), NICK);
    let m = message(&input);
    assert_eq!(
        m.emotes,
        vec![EmoteRange {
            id: "25".to_owned(),
            start: 0,
            end: 5
        }]
    );
    // No display-name: falls back to the login.
    assert_eq!(m.user.display_name, "u");
}

#[test]
fn shared_chat_source_room() {
    let line = "@id=s1;room-id=100000001;source-room-id=100000009;source-id=s0 :u!u@u.tmi.twitch.tv PRIVMSG #kinolune_demo :hi";
    let m = interpret(&parse_line(line).unwrap(), NICK);
    assert_eq!(message(&m).source_channel_id.as_deref(), Some("100000009"));
    let same = "@id=s2;room-id=100000001;source-room-id=100000001 :u!u@u.tmi.twitch.tv PRIVMSG #kinolune_demo :hi";
    let m = interpret(&parse_line(same).unwrap(), NICK);
    assert_eq!(message(&m).source_channel_id, None);
}
