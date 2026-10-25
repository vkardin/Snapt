//! Interface languages: English, Russian and Ukrainian.
//!
//! Strings live in one table, keyed by [`T`], so a missing translation is a
//! compile error rather than a blank label.

use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Ru,
    Uk,
}

impl Lang {
    pub const ALL: [Lang; 3] = [Lang::En, Lang::Ru, Lang::Uk];

    /// The code stored in the settings file.
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Ru => "ru",
            Lang::Uk => "uk",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        let code = code.trim().to_ascii_lowercase();
        Lang::ALL.into_iter().find(|l| code.starts_with(l.code()))
    }

    /// The language's own name, as shown in the language picker.
    pub fn name(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Ru => "Русский",
            Lang::Uk => "Українська",
        }
    }

    /// The system language when Snapt speaks it, otherwise English.
    pub fn system() -> Self {
        sys_locale::get_locale()
            .and_then(|l| Lang::from_code(&l))
            .unwrap_or(Lang::En)
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

pub fn set(lang: Lang) {
    CURRENT.store(lang as u8, Ordering::Relaxed);
}

pub fn current() -> Lang {
    match CURRENT.load(Ordering::Relaxed) {
        1 => Lang::Ru,
        2 => Lang::Uk,
        _ => Lang::En,
    }
}

/// Every string in the interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum T {
    // Tray
    TakeScreenshot,
    OpenFolder,
    SettingsMenu,
    About,
    Quit,
    Description,
    // Overlay
    Pen,
    Line,
    Arrow,
    Rectangle,
    Marker,
    Text,
    Color,
    Eyedropper,
    Undo,
    Close,
    Recognize,
    Recognizing,
    NoText,
    RecognizeFailed,
    OcrMissingWindows,
    OcrMissingLinux,
    Search,
    Print,
    Copy,
    Save,
    // Dialogs
    SaveTitle,
    PngImage,
    JpegImage,
    FolderTitle,
    // Browser pages
    PrintTitle,
    SearchTitle,
    SearchUploading,
    SearchFallback,
    // Settings window
    SettingsTitle,
    Language,
    SystemLanguage,
    StartAtLogin,
    Shortcut,
    PressShortcut,
    ShortcutTaken,
    ShortcutInvalid,
    Reset,
    Folder,
    Open,
    Format,
    QuickSave,
    ReplaceSystem,
    ReplaceHintWindows,
    ReplaceHintMac,
    ReplaceHintGnome,
    ReplaceHintLinux,
    ReplaceFailed,
    AutoUpdate,
    CheckNow,
    Checking,
    UpToDate,
    UpdateAvailable,
    InstallUpdate,
    Downloading,
    UpdateFailed,
    TrayUpdate,
    Website,
    // After the shot
    FullScreen,
    RepeatArea,
    Delayed,
    Seconds,
    History,
    HistoryEmpty,
    ClearHistory,
    Pin,
}

/// The current language's text for `key`.
pub fn t(key: T) -> &'static str {
    tr(current(), key)
}

pub fn tr(lang: Lang, key: T) -> &'static str {
    use Lang::*;
    use T::*;
    match (key, lang) {
        (TakeScreenshot, En) => "Take screenshot",
        (TakeScreenshot, Ru) => "Сделать скриншот",
        (TakeScreenshot, Uk) => "Зробити знімок екрана",
        (OpenFolder, En) => "Open screenshots folder",
        (OpenFolder, Ru) => "Открыть папку со скриншотами",
        (OpenFolder, Uk) => "Відкрити теку зі знімками",
        (SettingsMenu, En) => "Settings…",
        (SettingsMenu, Ru) => "Настройки…",
        (SettingsMenu, Uk) => "Налаштування…",
        (About, En) => "About Snapt",
        (About, Ru) => "О программе Snapt",
        (About, Uk) => "Про Snapt",
        (Quit, En) => "Quit Snapt",
        (Quit, Ru) => "Выйти из Snapt",
        (Quit, Uk) => "Вийти зі Snapt",
        (Description, En) => "A fast, lightweight, open-source screenshot tool.",
        (Description, Ru) => "Быстрая, лёгкая программа для скриншотов с открытым кодом.",
        (Description, Uk) => "Швидка й легка програма для знімків екрана з відкритим кодом.",

        (Pen, En) => "Pen",
        (Pen, Ru) => "Карандаш",
        (Pen, Uk) => "Олівець",
        (Line, En) => "Line",
        (Line, Ru) => "Линия",
        (Line, Uk) => "Лінія",
        (Arrow, En) => "Arrow",
        (Arrow, Ru) => "Стрелка",
        (Arrow, Uk) => "Стрілка",
        (Rectangle, En) => "Rectangle",
        (Rectangle, Ru) => "Прямоугольник",
        (Rectangle, Uk) => "Прямокутник",
        (Marker, En) => "Marker",
        (Marker, Ru) => "Маркер",
        (Marker, Uk) => "Маркер",
        (Text, En) => "Text",
        (Text, Ru) => "Текст",
        (Text, Uk) => "Текст",
        (Color, En) => "Color",
        (Color, Ru) => "Цвет",
        (Color, Uk) => "Колір",
        (Eyedropper, En) => "Pick a color from the screen",
        (Eyedropper, Ru) => "Взять цвет с экрана",
        (Eyedropper, Uk) => "Взяти колір з екрана",
        (Recognize, En) => "Copy text",
        (Recognize, Ru) => "Копировать текст",
        (Recognize, Uk) => "Копіювати текст",
        (Recognizing, En) => "Recognizing text…",
        (Recognizing, Ru) => "Распознаём текст…",
        (Recognizing, Uk) => "Розпізнаємо текст…",
        (NoText, En) => "No text found in the selection.",
        (NoText, Ru) => "В выделенной области нет текста.",
        (NoText, Uk) => "У виділеній області немає тексту.",
        (RecognizeFailed, En) => "Could not recognize the text.",
        (RecognizeFailed, Ru) => "Не удалось распознать текст.",
        (RecognizeFailed, Uk) => "Не вдалося розпізнати текст.",
        (OcrMissingWindows, En) => {
            "Add a language with text recognition in Windows Settings, Time & language."
        }
        (OcrMissingWindows, Ru) => {
            "Добавьте язык с распознаванием текста в Параметрах Windows, «Время и язык»."
        }
        (OcrMissingWindows, Uk) => {
            "Додайте мову з розпізнаванням тексту в Параметрах Windows, «Час і мова»."
        }
        (OcrMissingLinux, En) => {
            "Install tesseract-ocr (and tesseract-ocr-rus, -ukr) to recognize text."
        }
        (OcrMissingLinux, Ru) => {
            "Установите tesseract-ocr (и tesseract-ocr-rus, -ukr) для распознавания текста."
        }
        (OcrMissingLinux, Uk) => {
            "Встановіть tesseract-ocr (і tesseract-ocr-rus, -ukr) для розпізнавання тексту."
        }
        (Undo, En) => "Undo",
        (Undo, Ru) => "Отменить",
        (Undo, Uk) => "Скасувати",
        (Close, En) => "Close",
        (Close, Ru) => "Закрыть",
        (Close, Uk) => "Закрити",
        (Search, En) => "Search for similar images",
        (Search, Ru) => "Найти похожие изображения",
        (Search, Uk) => "Знайти схожі зображення",
        (Print, En) => "Print",
        (Print, Ru) => "Печать",
        (Print, Uk) => "Друк",
        (Copy, En) => "Copy",
        (Copy, Ru) => "Копировать",
        (Copy, Uk) => "Копіювати",
        (Save, En) => "Save",
        (Save, Ru) => "Сохранить",
        (Save, Uk) => "Зберегти",

        (SaveTitle, En) => "Save screenshot",
        (SaveTitle, Ru) => "Сохранить скриншот",
        (SaveTitle, Uk) => "Зберегти знімок",
        (PngImage, En) => "PNG image",
        (PngImage, Ru) => "Изображение PNG",
        (PngImage, Uk) => "Зображення PNG",
        (JpegImage, En) => "JPEG image",
        (JpegImage, Ru) => "Изображение JPEG",
        (JpegImage, Uk) => "Зображення JPEG",
        (FolderTitle, En) => "Screenshots folder",
        (FolderTitle, Ru) => "Папка для скриншотов",
        (FolderTitle, Uk) => "Тека для знімків",

        (PrintTitle, En) => "Print screenshot",
        (PrintTitle, Ru) => "Печать скриншота",
        (PrintTitle, Uk) => "Друк знімка",
        (SearchTitle, En) => "Search for similar images",
        (SearchTitle, Ru) => "Поиск похожих изображений",
        (SearchTitle, Uk) => "Пошук схожих зображень",
        (SearchUploading, En) => "Uploading to Google Lens…",
        (SearchUploading, Ru) => "Загружаем в Google Объектив…",
        (SearchUploading, Uk) => "Завантажуємо в Google Об'єктив…",
        (SearchFallback, En) => {
            "If nothing happens, open Google Lens and paste the screenshot: it is on the clipboard."
        }
        (SearchFallback, Ru) => {
            "Если ничего не происходит, откройте Google Объектив и вставьте скриншот: он в буфере обмена."
        }
        (SearchFallback, Uk) => {
            "Якщо нічого не відбувається, відкрийте Google Об'єктив і вставте знімок: він у буфері обміну."
        }

        (SettingsTitle, En) => "Snapt settings",
        (SettingsTitle, Ru) => "Настройки Snapt",
        (SettingsTitle, Uk) => "Налаштування Snapt",
        (Language, En) => "Language",
        (Language, Ru) => "Язык",
        (Language, Uk) => "Мова",
        (SystemLanguage, En) => "System",
        (SystemLanguage, Ru) => "Системный",
        (SystemLanguage, Uk) => "Системна",
        (StartAtLogin, En) => "Start at login",
        (StartAtLogin, Ru) => "Запускать при входе в систему",
        (StartAtLogin, Uk) => "Запускати під час входу",
        (Shortcut, En) => "Shortcut",
        (Shortcut, Ru) => "Горячая клавиша",
        (Shortcut, Uk) => "Гаряча клавіша",
        (PressShortcut, En) => "Press a shortcut…",
        (PressShortcut, Ru) => "Нажмите сочетание…",
        (PressShortcut, Uk) => "Натисніть сполучення…",
        (ShortcutTaken, En) => "This shortcut is taken by another app.",
        (ShortcutTaken, Ru) => "Это сочетание занято другой программой.",
        (ShortcutTaken, Uk) => "Це сполучення зайняте іншою програмою.",
        (ShortcutInvalid, En) => "Use a key with Ctrl, Alt or Shift, or PrtSc or F1–F12.",
        (ShortcutInvalid, Ru) => {
            "Используйте клавишу с Ctrl, Alt или Shift, либо PrtSc или F1–F12."
        }
        (ShortcutInvalid, Uk) => "Використайте клавішу з Ctrl, Alt чи Shift, або PrtSc чи F1–F12.",
        (Reset, En) => "Reset",
        (Reset, Ru) => "Сбросить",
        (Reset, Uk) => "Скинути",
        (Folder, En) => "Folder",
        (Folder, Ru) => "Папка",
        (Folder, Uk) => "Тека",
        (Open, En) => "Open",
        (Open, Ru) => "Открыть",
        (Open, Uk) => "Відкрити",
        (Format, En) => "Format",
        (Format, Ru) => "Формат",
        (Format, Uk) => "Формат",
        (QuickSave, En) => "Save without asking",
        (QuickSave, Ru) => "Сохранять без вопросов",
        (QuickSave, Uk) => "Зберігати без запитань",
        (ReplaceSystem, En) => "Replace system screenshots",
        (ReplaceSystem, Ru) => "Заменить системные скриншоты",
        (ReplaceSystem, Uk) => "Замінити системні знімки",
        (ReplaceHintWindows, En) => {
            "PrtSc stops opening the Snipping Tool. Sign out and back in if it still does."
        }
        (ReplaceHintWindows, Ru) => {
            "PrtSc перестанет открывать «Ножницы». Если открывает, выйдите из системы и войдите снова."
        }
        (ReplaceHintWindows, Uk) => {
            "PrtSc перестане відкривати «Ножиці». Якщо відкриває, вийдіть із системи й увійдіть знову."
        }
        (ReplaceHintMac, En) => {
            "Turn off Screenshots in Keyboard Shortcuts, then set Snapt to Cmd+Shift+3, 4 or 5."
        }
        (ReplaceHintMac, Ru) => {
            "Отключите «Снимки экрана» в сочетаниях клавиш, затем задайте Snapt Cmd+Shift+3, 4 или 5."
        }
        (ReplaceHintMac, Uk) => {
            "Вимкніть «Знімки екрана» в сполученнях клавіш, потім задайте Snapt Cmd+Shift+3, 4 або 5."
        }
        (ReplaceHintGnome, En) => {
            "The shortcut opens Snapt instead of GNOME Screenshot, on Wayland too."
        }
        (ReplaceHintGnome, Ru) => {
            "Сочетание открывает Snapt вместо скриншотов GNOME, в том числе на Wayland."
        }
        (ReplaceHintGnome, Uk) => {
            "Сполучення відкриває Snapt замість знімків GNOME, зокрема на Wayland."
        }
        (ReplaceHintLinux, En) => {
            "Turn off PrtSc in your desktop's keyboard settings and bind it to snapt --capture."
        }
        (ReplaceHintLinux, Ru) => {
            "Отключите PrtSc в настройках клавиатуры и назначьте на него snapt --capture."
        }
        (ReplaceHintLinux, Uk) => {
            "Вимкніть PrtSc у налаштуваннях клавіатури й призначте на нього snapt --capture."
        }
        (ReplaceFailed, En) => "Could not change the system setting.",
        (ReplaceFailed, Ru) => "Не удалось изменить системную настройку.",
        (ReplaceFailed, Uk) => "Не вдалося змінити системне налаштування.",
        (AutoUpdate, En) => "Check for updates",
        (AutoUpdate, Ru) => "Проверять обновления",
        (AutoUpdate, Uk) => "Перевіряти оновлення",
        (CheckNow, En) => "Check now",
        (CheckNow, Ru) => "Проверить",
        (CheckNow, Uk) => "Перевірити",
        (Checking, En) => "Checking…",
        (Checking, Ru) => "Проверяем…",
        (Checking, Uk) => "Перевіряємо…",
        (UpToDate, En) => "You have the latest version.",
        (UpToDate, Ru) => "У вас последняя версия.",
        (UpToDate, Uk) => "У вас остання версія.",
        (UpdateAvailable, En) => "Snapt {} is available.",
        (UpdateAvailable, Ru) => "Доступна версия Snapt {}.",
        (UpdateAvailable, Uk) => "Доступна версія Snapt {}.",
        (InstallUpdate, En) => "Update",
        (InstallUpdate, Ru) => "Обновить",
        (InstallUpdate, Uk) => "Оновити",
        (Downloading, En) => "Downloading the update…",
        (Downloading, Ru) => "Скачиваем обновление…",
        (Downloading, Uk) => "Завантажуємо оновлення…",
        (UpdateFailed, En) => "Could not reach GitHub. Try again later.",
        (UpdateFailed, Ru) => "Не удалось связаться с GitHub. Попробуйте позже.",
        (UpdateFailed, Uk) => "Не вдалося зв'язатися з GitHub. Спробуйте пізніше.",
        (TrayUpdate, En) => "Update to Snapt {}",
        (TrayUpdate, Ru) => "Обновить до Snapt {}",
        (TrayUpdate, Uk) => "Оновити до Snapt {}",
        (Website, En) => "Website",
        (Website, Ru) => "Сайт",
        (Website, Uk) => "Сайт",

        (FullScreen, En) => "Copy full screen",
        (FullScreen, Ru) => "Скопировать весь экран",
        (FullScreen, Uk) => "Скопіювати весь екран",
        (RepeatArea, En) => "Repeat last area",
        (RepeatArea, Ru) => "Повторить прошлую область",
        (RepeatArea, Uk) => "Повторити минулу область",
        (Delayed, En) => "Screenshot with delay",
        (Delayed, Ru) => "Скриншот с задержкой",
        (Delayed, Uk) => "Знімок із затримкою",
        (Seconds, En) => "In {} seconds",
        (Seconds, Ru) => "Через {} с",
        (Seconds, Uk) => "Через {} с",
        (History, En) => "Recent screenshots",
        (History, Ru) => "Последние скриншоты",
        (History, Uk) => "Останні знімки",
        (HistoryEmpty, En) => "No screenshots yet",
        (HistoryEmpty, Ru) => "Пока пусто",
        (HistoryEmpty, Uk) => "Поки порожньо",
        (ClearHistory, En) => "Clear history",
        (ClearHistory, Ru) => "Очистить историю",
        (ClearHistory, Uk) => "Очистити історію",
        (Pin, En) => "Pin to screen",
        (Pin, Ru) => "Закрепить поверх окон",
        (Pin, Uk) => "Закріпити поверх вікон",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_codes_round_trip() {
        for lang in Lang::ALL {
            assert_eq!(Lang::from_code(lang.code()), Some(lang));
        }
        assert_eq!(Lang::from_code("ru-RU"), Some(Lang::Ru));
        assert_eq!(Lang::from_code("uk_UA.UTF-8"), Some(Lang::Uk));
        assert_eq!(Lang::from_code("de-DE"), None);
    }

    #[test]
    fn the_bundled_font_covers_every_translation() {
        // Inter's subset holds Latin and Cyrillic; a glyph outside it would draw as a box.
        for lang in Lang::ALL {
            for key in [
                T::Search,
                T::SearchFallback,
                T::StartAtLogin,
                T::ShortcutInvalid,
            ] {
                for c in tr(lang, key).chars() {
                    assert!(crate::font::has_glyph(c), "{c:?} in {key:?} ({lang:?})");
                }
            }
        }
    }
}
