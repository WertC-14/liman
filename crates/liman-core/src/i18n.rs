//! Interface language. Texts are written in English in the code and looked up here, so every
//! translation lives in one table ([`turkish`]). An English text without a translation stays English.
//!
//! `tr("Open")` for a fixed text, `trf("{} items", &[&n])` for one with values (`{}` in order).

use std::fmt::Display;
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Tr,
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

pub fn set(lang: Lang) {
    CURRENT.store(lang as u8, Ordering::Relaxed);
}

pub fn current() -> Lang {
    match CURRENT.load(Ordering::Relaxed) {
        1 => Lang::Tr,
        _ => Lang::En,
    }
}

/// `tr`, `tr_TR.UTF-8` → Turkish; anything else English.
pub fn parse(name: &str) -> Option<Lang> {
    let lower = name.to_ascii_lowercase();
    if lower.starts_with("tr") {
        Some(Lang::Tr)
    } else if lower.starts_with("en") || lower == "c" || lower == "posix" {
        Some(Lang::En)
    } else {
        None
    }
}

/// Language from the locale variables, in the order the C library reads them.
pub fn from_env(get: impl Fn(&str) -> Option<String>) -> Lang {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|v| get(v).filter(|s| !s.is_empty()))
        .find_map(|s| parse(&s))
        .unwrap_or(Lang::En)
}

/// The text in the current language.
pub fn tr(en: &'static str) -> &'static str {
    translate(current(), en)
}

/// The text in the current language with `{}` filled in from `args`, in order.
pub fn trf(en: &'static str, args: &[&dyn Display]) -> String {
    fill(translate(current(), en), args)
}

pub fn translate(lang: Lang, en: &'static str) -> &'static str {
    match lang {
        Lang::En => en,
        Lang::Tr => turkish(en).unwrap_or(en),
    }
}

fn fill(template: &str, args: &[&dyn Display]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut args = args.iter();
    let mut rest = template;
    while let Some(i) = rest.find("{}") {
        out.push_str(&rest[..i]);
        if let Some(arg) = args.next() {
            out.push_str(&arg.to_string());
        }
        rest = &rest[i + 2..];
    }
    out.push_str(rest);
    out
}

/// Decimal separator of the current language (`13.8 kB` / `13,8 kB`).
pub fn decimal_comma() -> bool {
    current() == Lang::Tr
}

/// The Turkish table. Keys are the exact English texts used in the code.
pub fn turkish(en: &str) -> Option<&'static str> {
    Some(match en {
        // ---- sizes, counts, dates (liman-core::format)
        "1 byte" => "1 bayt",
        "{} bytes" => "{} bayt",
        "1 item" => "1 öğe",
        "{} items" => "{} öğe",
        "{}+ items" => "{}+ öğe",
        "Today {}" => "Bugün {}",
        "Yesterday {}" => "Dün {}",
        "{} days ago" => "{} gün önce",
        "Last week" => "Geçen hafta",
        "{} weeks ago" => "{} hafta önce",
        "Last month" => "Geçen ay",
        "{} months ago" => "{} ay önce",
        "Last year" => "Geçen yıl",
        "{} years ago" => "{} yıl önce",
        // ---- places
        "Home" => "Ev",
        "Trash" => "Çöp",
        "Recent" => "Son kullanılanlar",
        "Computer" => "Bilgisayar",
        // ---- jobs and their results (liman-core::job, ops)
        "Copied {}" => "{} kopyalandı",
        "Moved {}" => "{} taşındı",
        "Renamed “{}” to “{}”" => "“{}” → “{}” olarak adlandırıldı",
        "Moved {} to the trash" => "{} çöpe taşındı",
        "Undone: {}" => "Geri alındı: {}",
        "Deleted {} for good" => "{} kalıcı olarak silindi",
        "Created “{}”" => "“{}” oluşturuldu",
        "Cannot rename “{}”: {}" => "“{}” yeniden adlandırılamadı: {}",
        "Cannot undo: {}" => "Geri alınamadı: {}",
        "“{}” is not a valid name" => "“{}” geçerli bir ad değil",
        "“{}” already exists" => "“{}” zaten var",
        "{} already exists" => "{} zaten var",
        "cannot copy a folder into itself" => "bir klasör kendi içine kopyalanamaz",
        "cannot move a folder into itself" => "bir klasör kendi içine taşınamaz",
        // ---- file list and preview (liman-widgets)
        "Name" => "Ad",
        "Type" => "Tür",
        "Size" => "Boyut",
        "Modified" => "Değiştirme",
        "Folder" => "Klasör",
        "File" => "Dosya",
        "{} file" => "{} dosyası",
        "via {}" => "{} ile",
        "folder" | "folders" => "klasör",
        "code" => "kod",
        "config" => "ayar",
        "text" => "metin",
        "document" | "documents" => "belge",
        "sheet" | "sheets" => "tablo",
        "slide deck" | "slide decks" => "sunum",
        "archive" | "archives" => "arşiv",
        "image" | "images" => "resim",
        "video" | "videos" => "video",
        "audio" => "ses",
        "other" => "diğer",
        "Binary file" => "İkili dosya",
        "Cannot read this item" => "Bu öğe okunamıyor",
        "Cannot read: {}" => "Okunamadı: {}",
        "Cannot open this folder" => "Bu klasör açılamıyor",
        "Cannot show this image: {}" => "Bu resim gösterilemiyor: {}",
        "Install pdftotext (poppler) to see the text" => {
            "Metni görmek için pdftotext (poppler) kurun"
        }
        "Archive (no listing tool for this format)" => "Arşiv (bu biçim için listeleme aracı yok)",
        // ---- actions (palette, right-click menu)
        "Open" => "Aç",
        "Put path in terminal" => "Yolu terminale yaz",
        "Open terminal here" => "Burada terminal aç",
        "Copy" => "Kopyala",
        "Cut" => "Kes",
        "Paste" => "Yapıştır",
        "Rename" => "Yeniden adlandır",
        "New folder" => "Yeni klasör",
        "Copy path to clipboard" => "Yolu panoya kopyala",
        "Bookmark (toggle)" => "Yer imi (ekle/kaldır)",
        "Move to trash" => "Çöpe taşı",
        "Delete for good" => "Kalıcı olarak sil",
        "Mark all" => "Tümünü işaretle",
        "Undo" => "Geri al",
        "Search in subfolders" => "Alt klasörlerde ara",
        "Filter this folder" => "Bu klasörü süz",
        "Show / hide hidden files" => "Gizli dosyaları göster / gizle",
        "Sort by next column" => "Sonraki sütuna göre sırala",
        "Reverse sort order" => "Sıralamayı ters çevir",
        "Small list / large view" => "Küçük liste / büyük görünüm",
        "Zoom in" => "Büyüt",
        "Zoom out" => "Küçült",
        "Back" => "Geri",
        "Forward" => "İleri",
        "Parent folder" => "Üst klasör",
        "Recent files" => "Son kullanılan dosyalar",
        "Preview panel" => "Önizleme paneli",
        "New tab" => "Yeni sekme",
        " Reader · ↑↓ PgUp/PgDn scroll · Enter back to the panel · Esc close " => {
            " Okuyucu · ↑↓ PgUp/PgDn kaydır · Enter panele dön · Esc kapat "
        }
        "scroll" => "kaydır",
        "start / end" => "baş / son",
        "full screen" => "tam ekran",
        "files" => "dosyalar",
        "read the file full screen" => "dosyayı tam ekran oku",
        "Read file" => "Dosyayı oku",
        "Close tab" => "Sekmeyi kapat",
        "Next tab" => "Sonraki sekme",
        "This is the last tab (q quits)" => "Bu son sekme (q çıkar)",
        "new tab / close tab" => "yeni sekme / sekmeyi kapat",
        "go to tab" => "sekmeye geç",
        "Terminal panel" => "Terminal paneli",
        "Terminal full screen" => "Terminal tam ekran",
        "Theme…" => "Tema…",
        "All keys" => "Tüm kısayollar",
        "Quit" => "Çık",
        "Git: panel (changes, diff)" => "Git: panel (değişiklikler, diff)",
        "Git: stage" => "Git: hazırla (stage)",
        "Git: unstage" => "Git: hazırlıktan çıkar (unstage)",
        "Git: discard changes" => "Git: değişiklikleri at",
        "Git: commit" => "Git: commit",
        "Git: push" => "Git: push",
        "Git: pull" => "Git: pull",
        "Git: switch branch" => "Git: dal değiştir",
        // ---- messages (status bar)
        "Creating a folder" => "Klasör oluşturuluyor",
        "Copied path {}" => "Yol kopyalandı: {}",
        "Copied {} paths" => "{} yol kopyalandı",
        "{} ready to copy: open a folder and press Ctrl+V" => {
            "{} kopyalanmaya hazır: bir klasör açıp Ctrl+V'ye basın"
        }
        "{} ready to move: open a folder and press Ctrl+V" => {
            "{} taşınmaya hazır: bir klasör açıp Ctrl+V'ye basın"
        }
        "Nothing to paste: use Ctrl+C or Ctrl+X first" => {
            "Yapıştırılacak bir şey yok: önce Ctrl+C ya da Ctrl+X"
        }
        "Nothing to paste: every item already exists here" => {
            "Yapıştırılacak bir şey yok: hepsi zaten burada"
        }
        "Copying {}" => "{} kopyalanıyor",
        "Moving {}" => "{} taşınıyor",
        "Copying {} to “{}”" => "{} → “{}” kopyalanıyor",
        "Moving {} to “{}”" => "{} → “{}” taşınıyor",
        "Deleting {} for good" => "{} kalıcı olarak siliniyor",
        "These items are already in the trash" => "Bu öğeler zaten çöpte",
        "Moving {} to the trash" => "{} çöpe taşınıyor",
        "Renaming to “{}”" => "“{}” olarak adlandırılıyor",
        "Nothing to undo" => "Geri alınacak bir şey yok",
        "Undoing: {}" => "Geri alınıyor: {}",
        "Please wait: {} is still running" => "Lütfen bekleyin: {} sürüyor",
        "Failed: {}" => "Başarısız: {}",
        "{}, then failed: {}" => "{}, sonra başarısız: {}",
        "Not inside a git repository" => "Bir git deposunun içinde değil",
        "Staged" => "Hazırlandı",
        "Unstaged" => "Hazırlıktan çıkarıldı",
        "Discarded changes" => "Değişiklikler atıldı",
        "Nothing staged: stage files first (Space in the git panel)" => {
            "Hazırlanmış dosya yok: önce dosyaları hazırlayın (git panelinde Space)"
        }
        "Committed" => "Commit edildi",
        "Pushed" => "Push edildi",
        "Pulled" => "Pull edildi",
        "Switched branch" => "Dal değiştirildi",
        "Staged everything" => "Hepsi hazırlandı",
        "(binary or unreadable)" => "(ikili ya da okunamıyor)",
        "Cannot start a shell: {}" => "Kabuk başlatılamadı: {}",
        "The shell exited; F4 starts a new one" => "Kabuk kapandı; F4 yenisini açar",
        "Editor failed: {}" => "Düzenleyici hata verdi: {}",
        "No graphical display (SSH?): cannot open “{}” here" => {
            "Grafik ekran yok (SSH?): “{}” burada açılamıyor"
        }
        "Drop on a folder to move (hold Ctrl to copy)" => {
            "Taşımak için bir klasörün üstüne bırakın (kopyalamak için Ctrl)"
        }
        "Dropped outside a folder: nothing done" => "Klasör dışına bırakıldı: bir şey yapılmadı",
        "Theme: {}" => "Tema: {}",
        "No recent files" => "Son kullanılan dosya yok",
        "Removed bookmark “{}”" => "“{}” yer imlerinden çıkarıldı",
        "Bookmarked “{}”" => "“{}” yer imlerine eklendi",
        "Bookmark not saved: {}" => "Yer imi kaydedilemedi: {}",
        "Setting not saved: {}" => "Ayar kaydedilemedi: {}",
        "Showing hidden files" => "Gizli dosyalar gösteriliyor",
        "Hiding hidden files" => "Gizli dosyalar gizleniyor",
        "Sorted by {}" => "Sıralama: {}",
        "Sorted by {}, descending" => "Sıralama: {}, azalan",
        "name" => "ad",
        "size" => "boyut",
        "modified" => "tarih",
        "type" => "tür",
        "Opening “{}”…" => "“{}” açılıyor…",
        "Cannot open “{}”: {}" => "“{}” açılamadı: {}",
        "search “{}”" => "arama “{}”",
        // ---- screen (titles, dialogs, status bar)
        " Theme · ↑↓ preview · Enter keep · Esc cancel " => {
            " Tema · ↑↓ önizle · Enter seç · Esc vazgeç "
        }
        "{} already here:" => "{} zaten burada:",
        "  … and {} more" => "  … ve {} tane daha",
        "keep both (Enter)   " => "ikisini de tut (Enter)   ",
        "replace (old to trash)   " => "üzerine yaz (eskisi çöpe)   ",
        "skip   " => "atla   ",
        " Paste " => " Yapıştır ",
        "Throw away the changes in {}?" => "{} içindeki değişiklikler atılsın mı?",
        "Tracked files go back to the last commit; new files go to the trash." => {
            "İzlenen dosyalar son commit'e döner; yeni dosyalar çöpe gider."
        }
        "discard   " => "at   ",
        " Git: discard " => " Git: değişiklikleri at ",
        "Delete {} for good?" => "{} kalıcı olarak silinsin mi?",
        "This cannot be undone (Del moves to the trash instead)." => {
            "Bu geri alınamaz (Del ise çöpe taşır)."
        }
        "delete   " => "sil   ",
        " Delete for good " => " Kalıcı olarak sil ",
        " Space stage/unstage  a stage all  c commit  d discard  p push  P pull  b branch  Enter show  Esc close" => {
            " Space hazırla/çıkar  a hepsi  c commit  d at  p push  P pull  b dal  Enter göster  Esc kapat"
        }
        "Working tree clean" => "Çalışma ağacı temiz",
        " Switch branch · Enter switch · Esc " => " Dal değiştir · Enter geç · Esc ",
        " Commands · type to search · Enter run · Esc close " => {
            " Komutlar · aramak için yazın · Enter çalıştır · Esc kapat "
        }
        " No matching command" => " Eşleşen komut yok",
        " Keys · any key closes " => " Kısayollar · herhangi bir tuş kapatır ",
        " Places " => " Yerler ",
        " Results · {} " => " Sonuçlar · {} ",
        " Preview " => " Önizleme ",
        "  ·  {} found in {}" => "  ·  {} bulundu: {}",
        "   Bksp/Esc back to the folder" => "   Bksp/Esc klasöre dön",
        " ⎇ {} · {} changed  Ctrl+G " => " ⎇ {} · {} değişiklik  Ctrl+G ",
        "Loading…" => "Yükleniyor…",
        "Cannot open this folder: {}" => "Bu klasör açılamıyor: {}",
        "Folder is empty" => "Klasör boş",
        "Nothing matches “{}”" => "“{}” ile eşleşen yok",
        " Terminal · Ctrl+O back to files " => " Terminal · Ctrl+O dosyalara dön ",
        " Terminal · Tab on empty line: next panel · Ctrl+↑↓ size · F4 close · Ctrl+O full screen " => {
            " Terminal · boş satırda Tab: sonraki panel · Ctrl+↑↓ boyut · F4 kapat · Ctrl+O tam ekran "
        }
        " Terminal · Tab or click to type " => " Terminal · yazmak için Tab ya da tıklayın ",
        " ⎇ Commit message: " => " ⎇ Commit mesajı: ",
        "▏   Enter commit · Esc cancel" => "▏   Enter commit · Esc vazgeç",
        " ⌕ Search in this folder and below: " => " ⌕ Bu klasörde ve altında ara: ",
        "▏   Enter search · Esc cancel" => "▏   Enter ara · Esc vazgeç",
        " Rename: " => " Yeni ad: ",
        "rename " => "adlandır ",
        "cancel" => "vazgeç",
        " | “{}” selected{}" => " | “{}” seçili{}",
        " | {} marked" => " | {} işaretli",
        " | {} copied" => " | {} kopyalandı",
        " | {} cut" => " | {} kesildi",
        "Grid {}" => "Izgara {}",
        "Detailed" => "Ayrıntılı",
        "Normal" => "Normal",
        " | {} view" => " | {} görünüm",
        // status bar hints and the help window (keys, then what they do)
        "open" => "aç",
        "up" => "yukarı",
        "panels" => "paneller",
        "small/large" => "küçük/büyük",
        "theme" => "tema",
        "commands" => "komutlar",
        "all keys" => "tüm kısayollar",
        "quit" => "çık",
        "keep filter" => "süzgeci tut",
        "clear" => "temizle",
        "files / full screen" => "dosyalar / tam ekran",
        "focus" => "odak",
        "panel" => "panel",
        "Enter / double click" => "Enter / çift tık",
        "parent folder" => "üst klasör",
        "back / forward" => "geri / ileri",
        "Places · Files · Terminal" => "Yerler · Dosyalar · Terminal",
        "small list ↔ large view" => "küçük liste ↔ büyük görünüm",
        "+ / -  (Ctrl+wheel)" => "+ / -  (Ctrl+tekerlek)",
        "zoom" => "yakınlaştır",
        "filter" => "süz",
        "search in subfolders" => "alt klasörlerde ara",
        "bookmark folder (again: remove)" => "klasörü yer imine ekle (tekrar: kaldır)",
        "hidden files" => "gizli dosyalar",
        "s / S  (header click)" => "s / S  (başlığa tık)",
        "sort by / reverse" => "sırala / ters çevir",
        "mark / mark all" => "işaretle / tümünü işaretle",
        "Ctrl+click / Shift+click" => "Ctrl+tık / Shift+tık",
        "mark one / mark a range" => "birini / aralığı işaretle",
        "drag onto a folder" => "klasörün üstüne sürükle",
        "move (hold Ctrl: copy)" => "taşı (Ctrl basılı: kopyala)",
        "copy  cut  paste" => "kopyala  kes  yapıştır",
        "trash / rename / undo" => "çöp / yeniden adlandır / geri al",
        "delete for good (asks first)" => "kalıcı sil (önce sorar)",
        "preview panel" => "önizleme paneli",
        "terminal panel / full screen" => "terminal paneli / tam ekran",
        "selected paths into the terminal" => "seçili yolları terminale yaz",
        "terminal size" => "terminal boyutu",
        "Ctrl+P / right click" => "Ctrl+P / sağ tık",
        "all commands / menu" => "tüm komutlar / menü",
        "new folder / copy path (works over SSH)" => "yeni klasör / yolu kopyala (SSH'de de)",
        "git: changes, diff, stage, commit, push, pull, branch" => {
            "git paneli: diff, stage, commit, push, pull, dal"
        }
        "home" => "ev",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_names() {
        assert_eq!(parse("tr_TR.UTF-8"), Some(Lang::Tr));
        assert_eq!(parse("en_US.UTF-8"), Some(Lang::En));
        assert_eq!(parse("C"), Some(Lang::En));
        assert_eq!(parse("de_DE"), None);
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |k: &str| {
                pairs
                    .iter()
                    .find(|(n, _)| *n == k)
                    .map(|(_, v)| v.to_string())
            }
        };
        assert_eq!(from_env(env(&[("LANG", "tr_TR.UTF-8")])), Lang::Tr);
        assert_eq!(
            from_env(env(&[("LC_ALL", "C"), ("LANG", "tr_TR.UTF-8")])),
            Lang::En
        );
        assert_eq!(from_env(env(&[("LANG", "de_DE.UTF-8")])), Lang::En);
    }

    #[test]
    fn fill_and_fallback() {
        assert_eq!(translate(Lang::Tr, "{} items"), "{} öğe");
        assert_eq!(translate(Lang::Tr, "no such text"), "no such text");
        assert_eq!(fill("{} → {}", &[&1, &"b"]), "1 → b");
        assert_eq!(fill("no args {}", &[]), "no args ");
    }

    /// Every text passed to `tr`/`trf` anywhere in the workspace has a Turkish translation.
    #[test]
    fn every_wrapped_text_is_translated() {
        let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let mut missing = Vec::new();
        let mut stack = vec![crates];
        while let Some(dir) = stack.pop() {
            for item in std::fs::read_dir(&dir).unwrap().flatten() {
                let path = item.path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    let text = std::fs::read_to_string(&path).unwrap();
                    for key in wrapped_texts(&text) {
                        if turkish(&key).is_none() {
                            missing.push(format!("{}: {key}", path.display()));
                        }
                    }
                }
            }
        }
        assert!(
            missing.is_empty(),
            "no Turkish for:\n{}",
            missing.join("\n")
        );
    }

    /// String literals right after `tr(` or `trf(` (whitespace allowed; `\"` not used in keys).
    fn wrapped_texts(source: &str) -> Vec<String> {
        let mut out = Vec::new();
        // Built at run time so this function does not find its own text.
        for name in ["tr", "trf"] {
            let opener = format!("{name}(");
            let mut rest = source;
            while let Some(i) = rest.find(&opener) {
                let preceded_by_ident = rest[..i]
                    .chars()
                    .next_back()
                    .is_some_and(|c| c.is_alphanumeric() || c == '_');
                rest = rest[i + opener.len()..].trim_start();
                let Some(literal) = rest.strip_prefix('"') else {
                    continue;
                };
                let Some(end) = literal.find('"') else { break };
                if !preceded_by_ident {
                    out.push(literal[..end].to_string());
                }
                rest = &literal[end..];
            }
        }
        out
    }
}
