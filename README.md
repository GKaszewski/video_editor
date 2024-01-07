# video_editor

For now it is not an 'editor' per be but more like a tool that allows you to combine multiple video files into one, and merge audio tracks together and also
to set volume of the first track. I made it for my own purposes only. I needed a way to automize my workflow with OBS :)
In the future I am planning to add video and audio playback and new feature like cutting videos.

## usage

### CLI

```bash
video_editor -i "2024-01-07_04-45-38.mkv" -i "2024-01-07_04-45-46.mkv" -o "test.mp4" -c
```

--help for help
-i or --input for input
-o or --output for output
-c or --cli-mode for cli
-v or --volume for volume

### GUI

`Ctrl`+`I` to import videos
Click combine button to combine and set the output path

## technology

- Rust
- clap
- ffmpeg (needs to be installed)
- fltk
