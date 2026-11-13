# SPDX-License-Identifier: MPL-2.0
"""Text payloads for synthetic subtitles, chapters, CUE sheets and lyrics (all CC0)."""

from __future__ import annotations

LINES = {
    "en": ["[door creaks] Hello?", "Is anybody <i>there</i>?", "Line one\nand line two."],
    "ru": ["Привет, это тест субтитров.", "Ёлки-палки: «кавычки» и — тире.", "Первая строка\nвторая строка."],
    "forced": ["[НАДПИСЬ: «Выход»]", "[ТЕКСТ НА ЭКРАНЕ]", "[ПИСЬМО: «Встретимся в 9»]"],
}
TIMES = [(0.5, 1.5), (2.0, 3.0), (3.5, 4.5)]


def _ts(t: float, sep: str = ",") -> str:
    ms = round(t * 1000)
    return f"{ms // 3600000:02}:{ms // 60000 % 60:02}:{ms // 1000 % 60:02}{sep}{ms % 1000:03}"


def srt(kind: str) -> str:
    out = []
    for i, ((a, b), text) in enumerate(zip(TIMES, LINES[kind]), 1):
        out.append(f"{i}\n{_ts(a)} --> {_ts(b)}\n{text}\n")
    return "\n".join(out)


def vtt() -> str:
    out = ["WEBVTT", ""]
    for (a, b), text in zip(TIMES, ["Guten Tag.", "<b>Fett</b> und <i>kursiv</i>.", "Zeile eins\nZeile zwei."]):
        out += [f"{_ts(a, '.')} --> {_ts(b, '.')}", text, ""]
    return "\n".join(out)


def ass() -> str:
    """Styles, positioning, fades, karaoke and CJK text (font fallback)."""
    return """[Script Info]
; Synthetic test script (CC0), project_alpha QA-002
ScriptType: v4.00+
PlayResX: 1280
PlayResY: 720
WrapStyle: 0
ScaledBorderAndShadow: yes

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Default,DejaVu Sans,48,&H00FFFFFF,&H000000FF,&H00000000,&H80000000,0,0,0,0,100,100,0,0,1,3,1,2,40,40,40,1
Style: Sign,DejaVu Sans,36,&H0000FFFF,&H000000FF,&H00202020,&H00000000,1,0,0,0,100,100,0,-8,1,2,0,7,10,10,10,1
Style: Karaoke,DejaVu Sans,44,&H00FFFFFF,&H00FF8000,&H00000000,&H00000000,1,0,0,0,100,100,0,0,1,2,0,8,10,10,30,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 0,0:00:00.50,0:00:01.50,Default,,0,0,0,,{\\fad(200,200)}Обычная реплика с затуханием
Dialogue: 0,0:00:00.50,0:00:04.50,Sign,,0,0,0,,{\\pos(200,120)\\frz-8}ВЫВЕСКА: «Аптека»
Dialogue: 0,0:00:02.00,0:00:03.00,Karaoke,,0,0,0,,{\\k25}Ka{\\k25}ra{\\k25}o{\\k25}ke
Dialogue: 0,0:00:03.50,0:00:04.50,Default,,0,0,0,,日本語のテキスト (fallback font)
"""


def ffmetadata_chapters(duration: float, titles: list[str]) -> str:
    step = duration / len(titles)
    out = [";FFMETADATA1"]
    for i, title in enumerate(titles):
        out += ["[CHAPTER]", "TIMEBASE=1/1000", f"START={round(i * step * 1000)}",
                f"END={round((i + 1) * step * 1000)}", f"title={title}"]
    return "\n".join(out) + "\n"


def cue(file_name: str) -> str:
    return f"""REM GENRE Test
REM DATE 2024
PERFORMER "Синтетика"
TITLE "CUE Album"
FILE "{file_name}" WAVE
  TRACK 01 AUDIO
    TITLE "Первый"
    INDEX 01 00:00:00
  TRACK 02 AUDIO
    TITLE "Second"
    INDEX 00 00:02:50
    INDEX 01 00:03:00
  TRACK 03 AUDIO
    TITLE "Третий"
    INDEX 01 00:06:00
"""


def lrc() -> str:
    return """[ar:Синтетика]
[ti:Song]
[al:Lyrics]
[length:00:06.00]
[00:00.50]Первая строка
[00:02.00]Second line
[00:04.00]<00:04.00>Сло<00:04.40>во <00:04.80>за <00:05.20>словом
"""
