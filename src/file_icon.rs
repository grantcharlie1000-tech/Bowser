//! The picture of a downloaded file (docs/design/downloads.md "File icon"): the icon Windows shows for
//! it in Explorer, as a small PNG `data:` URL for the downloads bubble and page.
//!
//! A finished program (`.exe`, `.lnk`, ...) carries its own icon, read from the file itself; any other
//! file, and a program still downloading, gets the icon of its type (the app that opens `.pdf`, `.zip`,
//! ...), which needs no file at all. Icons are cached: a type is looked up once, a program once per file.
//! The shell lookup is a few milliseconds for a local file and runs only for rows the chrome shows.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};

/// Largest PNG a file icon may be (a 32 x 32 icon is 1 to 4 KB; 48 x 48 at 150% scaling a little more).
pub const MAX_ICON_BYTES: usize = 16 * 1024;
/// How many icons the cache keeps.
pub const MAX_CACHED: usize = 128;
/// Longest extension that is looked up as a type ("torrent", "appinstaller" fit).
const MAX_EXT_CHARS: usize = 16;
/// Types whose files carry their own icon once complete. Everything else shows its type's icon.
const OWN_ICON: &[&str] = &["exe", "lnk", "ico", "scr", "cpl"];

/// What an icon is looked up by.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum IconKey {
    /// The icon inside this complete file (a program, a shortcut, an icon file).
    File(PathBuf),
    /// The icon of a type, by lower-case extension without the dot ("" = a file with no extension).
    Type(String),
}

/// The lower-case extension of `path` when it is a plain one (ASCII letters and digits, short), else "".
pub fn extension(path: &Path) -> String {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if ext.is_empty() || ext.len() > MAX_EXT_CHARS || !ext.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return String::new();
    }
    ext.to_ascii_lowercase()
}

/// The key for a download at `path`: its own icon when it is a complete program-like file, else its type's.
pub fn key_for(path: &Path, complete: bool) -> IconKey {
    let ext = extension(path);
    if complete && path.is_absolute() && OWN_ICON.contains(&ext.as_str()) {
        IconKey::File(path.to_path_buf())
    } else {
        IconKey::Type(ext)
    }
}

/// `data:image/png;base64,...` for PNG bytes small enough to send; None for anything else.
pub fn data_uri(png: &[u8]) -> Option<String> {
    const SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
    (png.len() > SIGNATURE.len() && png.len() <= MAX_ICON_BYTES && png.starts_with(SIGNATURE))
        .then(|| format!("data:image/png;base64,{}", crate::chrome_host::protocol::base64_encode(png)))
}

/// Looks an icon up: a `data:` URL, or None when there is none to show (the chrome then draws its glyph).
pub type IconSource = fn(&IconKey) -> Option<String>;

/// A bounded cache of looked-up icons, failures included (a file without an icon is not asked again).
#[derive(Default)]
pub struct IconCache {
    map: HashMap<IconKey, Option<String>>,
    order: VecDeque<IconKey>,
}

impl IconCache {
    /// The icon for `key`, from the cache or from `source` (then cached). A file whose own icon cannot be
    /// read falls back to its type's icon.
    pub fn get(&mut self, key: &IconKey, source: IconSource) -> Option<String> {
        if let Some(hit) = self.map.get(key) {
            return hit.clone();
        }
        let mut found = source(key);
        if found.is_none()
            && let IconKey::File(path) = key
        {
            found = self.get(&IconKey::Type(extension(path)), source);
        }
        self.put(key.clone(), found.clone());
        found
    }

    /// Forgets the icon of one file (it was replaced or deleted).
    pub fn forget(&mut self, key: &IconKey) {
        if self.map.remove(key).is_some() {
            self.order.retain(|k| k != key);
        }
    }

    fn put(&mut self, key: IconKey, value: Option<String>) {
        if self.map.insert(key.clone(), value).is_none() {
            self.order.push_back(key);
        }
        while self.order.len() > MAX_CACHED {
            if let Some(old) = self.order.pop_front() {
                self.map.remove(&old);
            }
        }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

/// The icon Windows shows for `key`, as a PNG `data:` URL. COM must be initialised on the calling thread,
/// as it is on the window thread.
#[cfg(windows)]
pub fn load(key: &IconKey) -> Option<String> {
    use windows::Win32::{
        Storage::FileSystem::{FILE_ATTRIBUTE_NORMAL, FILE_FLAGS_AND_ATTRIBUTES},
        UI::{
            Shell::{SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON, SHGFI_USEFILEATTRIBUTES, SHGetFileInfoW},
            WindowsAndMessaging::DestroyIcon,
        },
    };
    use windows::core::PCWSTR;

    let (name, attributes, flags) = match key {
        IconKey::File(path) => (path.as_os_str().to_owned(), FILE_FLAGS_AND_ATTRIBUTES(0), SHGFI_ICON | SHGFI_LARGEICON),
        // A made-up name of the type: with SHGFI_USEFILEATTRIBUTES the shell never touches the disk.
        IconKey::Type(ext) => {
            let name = if ext.is_empty() { "file".to_owned() } else { format!("file.{ext}") };
            (name.into(), FILE_ATTRIBUTE_NORMAL, SHGFI_ICON | SHGFI_LARGEICON | SHGFI_USEFILEATTRIBUTES)
        }
    };
    let wide: Vec<u16> = {
        use std::os::windows::ffi::OsStrExt;
        name.encode_wide().chain(std::iter::once(0)).collect()
    };
    let mut info = SHFILEINFOW::default();
    let ok = unsafe { SHGetFileInfoW(PCWSTR(wide.as_ptr()), attributes, Some(&mut info), std::mem::size_of::<SHFILEINFOW>() as u32, flags) };
    if ok == 0 || info.hIcon.is_invalid() {
        return None;
    }
    let png = icon_png(info.hIcon);
    unsafe {
        let _ = DestroyIcon(info.hIcon);
    }
    data_uri(&png?)
}

/// Encodes an icon (with its alpha channel) as a PNG through Windows Imaging Component.
#[cfg(windows)]
fn icon_png(icon: windows::Win32::UI::WindowsAndMessaging::HICON) -> Option<Vec<u8>> {
    use windows::Win32::{
        Graphics::Imaging::{
            CLSID_WICImagingFactory, GUID_ContainerFormatPng, GUID_WICPixelFormat32bppBGRA, IWICBitmapFrameEncode, IWICImagingFactory,
            WICBitmapEncoderNoCache,
        },
        System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, IStream, STATFLAG_NONAME, STATSTG, STREAM_SEEK_SET, StructuredStorage::IPropertyBag2},
        UI::Shell::SHCreateMemStream,
    };
    let factory = unsafe { CoCreateInstance::<_, IWICImagingFactory>(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()? };
    let bitmap = unsafe { factory.CreateBitmapFromHICON(icon).ok()? };
    let (mut width, mut height) = (0u32, 0u32);
    unsafe { bitmap.GetSize(&mut width, &mut height).ok()? };
    if width == 0 || height == 0 || width > 256 || height > 256 {
        return None;
    }
    let sink: IStream = unsafe { SHCreateMemStream(None) }?;
    let encoder = unsafe { factory.CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null()).ok()? };
    unsafe { encoder.Initialize(&sink, WICBitmapEncoderNoCache).ok()? };
    let mut frame: Option<IWICBitmapFrameEncode> = None;
    let mut options: Option<IPropertyBag2> = None;
    unsafe { encoder.CreateNewFrame(&mut frame, &mut options).ok()? };
    let frame = frame?;
    unsafe {
        frame.Initialize(options.as_ref()).ok()?;
        frame.SetSize(width, height).ok()?;
        let mut format = GUID_WICPixelFormat32bppBGRA;
        frame.SetPixelFormat(&mut format).ok()?;
        frame.WriteSource(&bitmap, std::ptr::null()).ok()?;
        frame.Commit().ok()?;
        encoder.Commit().ok()?;
    }
    let mut stat = STATSTG::default();
    unsafe { sink.Stat(&mut stat, STATFLAG_NONAME).ok()? };
    let len = usize::try_from(stat.cbSize).ok()?;
    if len == 0 || len > MAX_ICON_BYTES {
        return None;
    }
    unsafe { sink.Seek(0, STREAM_SEEK_SET, None).ok()? };
    let mut out = vec![0u8; len];
    let mut read = 0u32;
    unsafe { sink.Read(out.as_mut_ptr().cast(), len as u32, Some(&mut read)).ok().ok()? };
    out.truncate(read as usize);
    (out.len() == len).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn abs(name: &str) -> PathBuf {
        if cfg!(windows) { PathBuf::from(format!("C:\\Users\\me\\Downloads\\{name}")) } else { PathBuf::from(format!("/home/me/Downloads/{name}")) }
    }

    #[test]
    fn a_complete_program_uses_its_own_icon_and_everything_else_its_type() {
        assert_eq!(key_for(&abs("zen.installer.exe"), true), IconKey::File(abs("zen.installer.exe")));
        assert_eq!(key_for(&abs("Setup.EXE"), true), IconKey::File(abs("Setup.EXE")));
        assert_eq!(key_for(&abs("Shortcut.lnk"), true), IconKey::File(abs("Shortcut.lnk")));
        // still downloading: the file is incomplete, so its type's icon
        assert_eq!(key_for(&abs("zen.installer.exe"), false), IconKey::Type("exe".into()));
        assert_eq!(key_for(&abs("report.PDF"), true), IconKey::Type("pdf".into()));
        assert_eq!(key_for(&abs("archive.tar.gz"), true), IconKey::Type("gz".into()));
        assert_eq!(key_for(&abs("README"), true), IconKey::Type(String::new()));
        // a relative path is never read
        assert_eq!(key_for(Path::new("zen.exe"), true), IconKey::Type("exe".into()));
    }

    #[test]
    fn odd_extensions_are_looked_up_as_no_extension() {
        assert_eq!(extension(Path::new("a.toolongextensionname1")), "");
        assert_eq!(extension(Path::new("a.ex e")), "");
        assert_eq!(extension(Path::new("a.é")), "");
        assert_eq!(extension(Path::new("a.Mp4")), "mp4");
    }

    #[test]
    fn only_small_pngs_become_data_uris() {
        let png = [b"\x89PNG\r\n\x1a\n".as_slice(), &[0u8; 32]].concat();
        assert!(data_uri(&png).is_some_and(|u| u.starts_with("data:image/png;base64,iVBORw0KGgo")));
        assert_eq!(data_uri(b"GIF89a....."), None);
        assert_eq!(data_uri(b"\x89PNG\r\n\x1a\n"), None);
        let big = [b"\x89PNG\r\n\x1a\n".as_slice(), &vec![0u8; MAX_ICON_BYTES]].concat();
        assert_eq!(data_uri(&big), None);
    }

    thread_local! { static CALLS: Cell<u32> = const { Cell::new(0) }; }

    fn fake(key: &IconKey) -> Option<String> {
        CALLS.with(|c| c.set(c.get() + 1));
        match key {
            IconKey::File(path) if path.ends_with("good.exe") => Some("data:image/png;base64,FILE".into()),
            IconKey::File(_) => None,
            IconKey::Type(ext) if ext == "exe" => Some("data:image/png;base64,EXE".into()),
            IconKey::Type(_) => None,
        }
    }

    #[test]
    fn icons_are_looked_up_once_and_a_file_without_one_falls_back_to_its_type() {
        CALLS.with(|c| c.set(0));
        let mut cache = IconCache::default();
        let good = IconKey::File(abs("good.exe"));
        assert_eq!(cache.get(&good, fake).as_deref(), Some("data:image/png;base64,FILE"));
        assert_eq!(cache.get(&good, fake).as_deref(), Some("data:image/png;base64,FILE"));
        assert_eq!(CALLS.with(Cell::get), 1);
        let plain = IconKey::File(abs("no-icon.exe"));
        assert_eq!(cache.get(&plain, fake).as_deref(), Some("data:image/png;base64,EXE"));
        assert_eq!(cache.get(&plain, fake).as_deref(), Some("data:image/png;base64,EXE"));
        assert_eq!(CALLS.with(Cell::get), 3, "file miss, then the type, then cached");
        assert_eq!(cache.get(&IconKey::Type("zip".into()), fake), None);
        assert_eq!(cache.get(&IconKey::Type("zip".into()), fake), None);
        assert_eq!(CALLS.with(Cell::get), 4, "a type without an icon is not asked again");
        cache.forget(&good);
        assert_eq!(cache.get(&good, fake).as_deref(), Some("data:image/png;base64,FILE"));
        assert_eq!(CALLS.with(Cell::get), 5);
    }

    #[test]
    fn the_cache_is_bounded() {
        let mut cache = IconCache::default();
        for i in 0..(MAX_CACHED + 40) {
            cache.get(&IconKey::Type(format!("x{i}")), fake);
        }
        assert_eq!(cache.len(), MAX_CACHED);
    }
}
