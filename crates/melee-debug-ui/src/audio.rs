use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const SAMPLE_RATE: u32 = 32_000;
pub const CHANNELS: usize = 2;

fn sibling(out: &Path, suffix: &str) -> PathBuf {
    let mut name = out.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

pub fn track_path(out: &Path) -> PathBuf {
    sibling(out, ".audio.f32")
}

pub fn mux_path(out: &Path) -> PathBuf {
    let extension = out.extension().unwrap_or_else(|| OsStr::new("mp4"));
    let mut suffix = OsString::from(".mux.");
    suffix.push(extension);
    sibling(out, &suffix.to_string_lossy())
}

pub fn mux_args(video: &Path, track: &Path, muxed: &Path) -> Vec<OsString> {
    let mut args: Vec<OsString> = ["-hide_banner", "-loglevel", "error", "-y", "-i"]
        .iter()
        .map(OsString::from)
        .collect();
    args.push(video.into());
    args.extend(["-f", "f32le", "-ar"].iter().map(OsString::from));
    args.push(SAMPLE_RATE.to_string().into());
    args.push("-ac".into());
    args.push(CHANNELS.to_string().into());
    args.push("-i".into());
    args.push(track.into());
    args.extend(
        [
            "-map",
            "0:v:0",
            "-map",
            "1:a:0",
            "-c:v",
            "copy",
            "-c:a",
            "aac",
            "-b:a",
            "192k",
            "-movflags",
            "+faststart",
        ]
        .iter()
        .map(OsString::from),
    );
    args.push(muxed.into());
    args
}

pub struct AudioTrack {
    path: PathBuf,
    out: BufWriter<File>,
    frames: u64,
}

impl AudioTrack {
    pub fn create(path: PathBuf) -> io::Result<Self> {
        let out = BufWriter::new(File::create(&path)?);
        Ok(Self {
            path,
            out,
            frames: 0,
        })
    }

    pub fn write(&mut self, samples: &[f32]) -> io::Result<()> {
        self.out.write_all(bytemuck::cast_slice(samples))?;
        self.frames += (samples.len() / CHANNELS) as u64;
        Ok(())
    }

    pub fn frames(&self) -> u64 {
        self.frames
    }

    pub fn seconds(&self) -> f64 {
        self.frames as f64 / f64::from(SAMPLE_RATE)
    }

    pub fn finish(mut self) -> io::Result<PathBuf> {
        self.out.flush()?;
        Ok(self.path)
    }
}

pub fn mux(binary: &OsStr, video: &Path, track: &Path) -> io::Result<()> {
    let muxed = mux_path(video);
    let status = Command::new(binary)
        .args(mux_args(video, track, &muxed))
        .stdin(Stdio::null())
        .status()?;
    if !status.success() {
        let _ = fs::remove_file(&muxed);
        return Err(io::Error::other(format!(
            "{} exited with {status}",
            binary.to_string_lossy()
        )));
    }
    fs::rename(&muxed, video)?;
    fs::remove_file(track)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::fs;
    use std::path::{Path, PathBuf};

    use crate::audio::{AudioTrack, mux_args, mux_path, track_path};

    #[test]
    fn side_files_sit_next_to_the_output() {
        let out = Path::new("build/captures/run.mp4");
        assert_eq!(
            track_path(out),
            PathBuf::from("build/captures/run.mp4.audio.f32")
        );
        assert_eq!(
            mux_path(out),
            PathBuf::from("build/captures/run.mp4.mux.mp4")
        );
        assert_eq!(
            mux_path(Path::new("clip.mkv")),
            PathBuf::from("clip.mkv.mux.mkv")
        );
        assert_eq!(mux_path(Path::new("noext")), PathBuf::from("noext.mux.mp4"));
    }

    #[test]
    fn the_mux_copies_video_and_encodes_the_raw_track() {
        let args = mux_args(Path::new("v.mp4"), Path::new("a.f32"), Path::new("m.mp4"));
        let args: Vec<String> = args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args.join(" "),
            "-hide_banner -loglevel error -y -i v.mp4 -f f32le -ar 32000 -ac 2 -i a.f32 \
             -map 0:v:0 -map 1:a:0 -c:v copy -c:a aac -b:a 192k -movflags +faststart m.mp4"
        );
        assert_eq!(args.last(), Some(&"m.mp4".to_owned()));
        assert!(!args.contains(&OsString::from("-an").to_string_lossy().into_owned()));
    }

    #[test]
    fn a_track_is_raw_little_endian_f32_and_counts_stereo_frames() {
        let path =
            std::env::temp_dir().join(format!("melee-audio-test-{}.f32", std::process::id()));
        let mut track = AudioTrack::create(path.clone()).unwrap();
        track.write(&[0.0, 1.0, -1.0, 0.5]).unwrap();
        track.write(&[0.25, -0.25]).unwrap();
        assert_eq!(track.frames(), 3);
        assert!((track.seconds() - 3.0 / 32_000.0).abs() < 1e-12);
        assert_eq!(track.finish().unwrap(), path);
        let bytes = fs::read(&path).unwrap();
        fs::remove_file(&path).unwrap();
        let expected: Vec<u8> = [0.0_f32, 1.0, -1.0, 0.5, 0.25, -0.25]
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect();
        assert_eq!(bytes, expected);
    }

    #[test]
    fn an_unwritable_track_path_is_an_error() {
        assert!(AudioTrack::create(PathBuf::from("/nonexistent-dir/a.f32")).is_err());
    }
}
