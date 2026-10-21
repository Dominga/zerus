# Activity additions (WhiteWh/zerus)

None of this is part of upstream Zerus.

## Working line

While the selected session works, a line above the message field shows an
animated mark, what the agent is doing now and how long its turn has run, as
Claude Code shows it in its terminal. The text is the current step of the
agent's plan, else the tool it uses, else the activity it reports. The line is
hidden while the session is ready, asks a question or waits for an approval
(`tray/src/WorkingLine.h`).

## Pictures, animations and videos

When an agent's message names a picture (`.png`, `.jpg`, `.webp`, `.bmp`), an
animation (`.gif`) or a video (`.mp4`, `.webm`, `.mov`, `.mkv`, `.m4v`, `.avi`),
as a Markdown image or as a file reference, Activity shows a 320 x 180 preview
under the message: the picture, the running animation, or a video's first
frame with a play mark. A click opens it in Zerus: a picture at up to screen
size, or a player with pause and seeking. The viewer can also hand the file to
the default application (`tray/src/MediaViewer.h`).

Files of this computer are read where they are; files over 64 MiB get no
preview. A peer's files come through the download cache (`hgs @peer download`);
a peer's video loads only when clicked. Nothing is fetched from the web, and
pictures inside tool results that the agent never names stay out of Activity.
Only Qt Multimedia is used, as for the voice and video messages.

## Text selection

A skin whose selected rows are a faint tint of the canvas (Compact) selects
text in a mix of its accent and the canvas, so selected text stays readable.
