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
    PickColor,
    ColorFormat,
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
    General,
    Shortcuts,
    Features,
    Editor,
    EditorHint,
    ShortcutsHint,
    Ellipse,
    Number,
    Pixelate,
    Blur,
    Spotlight,
    Crop,
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
    PressShortcut,
    ShortcutTaken,
    ShortcutInvalid,
    Reset,
    Folder,
    Open,
    Format,
    QuickSave,
    CopyAs,
    CopyImage,
    CopyFile,
    CopyOnSelect,
    WindowOpensEditor,
    SaveOnCopy,
    OpenAfterSave,
    ReplaceSystem,
    ReplaceHintWindows,
    ReplaceHintMac,
    ReplaceHintGnome,
    ReplaceHintLinux,
    ReplaceFailed,
    ReplaceWhen,
    ReplaceAsk,
    ReplaceAuto,
    ReplaceQuestion,
    RestoreSystem,
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
    Updates,
    Example,
    CaptureCursor,
    HintCaptureCursor,
    SecCapture,
    SecSaving,
    SecSystem,
    SecAbout,
    SubGeneral,
    SubCapture,
    SubSaving,
    SubFeatures,
    SubSystem,
    Theme,
    ThemeSystem,
    ThemeLight,
    ThemeDark,
    HintColorFormat,
    HintCopyOnSelect,
    HintWindowOpensEditor,
    HintCopyAs,
    HintQuickSave,
    HintSaveOnCopy,
    FileName,
    HintFileName,
    HistoryLimit,
    Off,
    GroupAnywhere,
    GroupInEditor,
    ResetSection,
    ResetConfirm,
    SearchSettings,
    NothingFound,
    Version,
    License,
    MoreFromAuthor,
    LevelerAbout,
    VideoGlowAbout,
    HintRecognize,
    HintScroll,
    HintRecord,
    HintPin,
    HintSearch,
    HintPrint,
    HintPicker,
    // After the shot
    FullScreen,
    RepeatArea,
    PinLast,
    Delayed,
    Seconds,
    History,
    HistoryEmpty,
    ClearHistory,
    Pin,
    Scroll,
    ScrollHint,
    ScrollHeight,
    Record,
    RecordHint,
    RecordTime,
    RecordTitle,
    GifVideo,
    Mp4Video,
    // 3.0
    ReadQr,
    NoQr,
    HidePersonal,
    Redacting,
    NothingToHide,
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
        (PickColor, En) => "Pick a color",
        (PickColor, Ru) => "Пипетка",
        (PickColor, Uk) => "Піпетка",
        (ColorFormat, En) => "Color format",
        (ColorFormat, Ru) => "Формат цвета",
        (ColorFormat, Uk) => "Формат кольору",
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
        (General, En) => "General",
        (General, Ru) => "Основные",
        (General, Uk) => "Основні",
        (Shortcuts, En) => "Shortcuts",
        (Shortcuts, Ru) => "Клавиши",
        (Shortcuts, Uk) => "Клавіші",
        (Features, En) => "Features",
        (Features, Ru) => "Функции",
        (Features, Uk) => "Функції",
        (Editor, En) => "Editor",
        (Editor, Ru) => "Редактор",
        (Editor, Uk) => "Редактор",
        (EditorHint, En) => "Turned-off tools leave the toolbar and their keys stop working.",
        (EditorHint, Ru) => "Выключенные инструменты пропадают с панели, их клавиши не работают.",
        (EditorHint, Uk) => "Вимкнені інструменти зникають з панелі, їхні клавіші не працюють.",
        (ShortcutsHint, En) => "Click a field and press the keys. Backspace clears it.",
        (ShortcutsHint, Ru) => "Нажмите на поле и затем на клавиши. Backspace очищает.",
        (ShortcutsHint, Uk) => "Натисніть на поле, а потім на клавіші. Backspace очищає.",
        (Rectangle, En) => "Rectangle",
        (Rectangle, Ru) => "Прямоугольник",
        (Rectangle, Uk) => "Прямокутник",
        (Ellipse, En) => "Ellipse",
        (Ellipse, Ru) => "Эллипс",
        (Ellipse, Uk) => "Еліпс",
        (Number, En) => "Numbered step",
        (Number, Ru) => "Нумерация",
        (Number, Uk) => "Нумерація",
        (Pixelate, En) => "Pixelate",
        (Pixelate, Ru) => "Пикселизация",
        (Pixelate, Uk) => "Пікселізація",
        (Blur, En) => "Blur",
        (Blur, Ru) => "Размытие",
        (Blur, Uk) => "Розмиття",
        (Spotlight, En) => "Spotlight",
        (Spotlight, Ru) => "Прожектор",
        (Spotlight, Uk) => "Прожектор",
        (Crop, En) => "Crop",
        (Crop, Ru) => "Обрезать",
        (Crop, Uk) => "Обрізати",
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
        (CopyAs, En) => "Copy as",
        (CopyAs, Ru) => "Копировать как",
        (CopyAs, Uk) => "Копіювати як",
        (CopyImage, En) => "Image",
        (CopyImage, Ru) => "Картинку",
        (CopyImage, Uk) => "Зображення",
        (CopyFile, En) => "File",
        (CopyFile, Ru) => "Файл",
        (CopyFile, Uk) => "Файл",
        (CopyOnSelect, En) => "Copy right after selecting",
        (CopyOnSelect, Ru) => "Копировать сразу после выделения",
        (CopyOnSelect, Uk) => "Копіювати одразу після виділення",
        (WindowOpensEditor, En) => "Clicking a window opens the editor",
        (WindowOpensEditor, Ru) => "Клик по окну открывает редактор",
        (WindowOpensEditor, Uk) => "Клік по вікну відкриває редактор",
        (SaveOnCopy, En) => "Also save what I copy",
        (SaveOnCopy, Ru) => "Сохранять и скопированное",
        (SaveOnCopy, Uk) => "Зберігати й скопійоване",
        (OpenAfterSave, En) => "Open the file after saving",
        (OpenAfterSave, Ru) => "Открывать файл после сохранения",
        (OpenAfterSave, Uk) => "Відкривати файл після збереження",
        (ReplaceSystem, En) => "Replace system screenshots",
        (ReplaceSystem, Ru) => "Заменить системные скриншоты",
        (ReplaceSystem, Uk) => "Замінити системні знімки",
        (ReplaceHintWindows, En) => {
            "PrtSc and Win+Shift+S open Snapt. Windows asks you to confirm Snapt for screenshot links."
        }
        (ReplaceHintWindows, Ru) => {
            "PrtSc и Win+Shift+S открывают Snapt. Для ссылок на скриншоты Windows попросит подтвердить выбор."
        }
        (ReplaceHintWindows, Uk) => {
            "PrtSc і Win+Shift+S відкривають Snapt. Для посилань на знімки Windows попросить підтвердити вибір."
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
        (ReplaceWhen, En) => "When Windows takes it back",
        (ReplaceWhen, Ru) => "Если Windows вернёт своё",
        (ReplaceWhen, Uk) => "Якщо Windows поверне своє",
        (ReplaceAsk, En) => "Ask",
        (ReplaceAsk, Ru) => "Спросить",
        (ReplaceAsk, Uk) => "Запитати",
        (ReplaceAuto, En) => "Replace",
        (ReplaceAuto, Ru) => "Заменить",
        (ReplaceAuto, Uk) => "Замінити",
        (RestoreSystem, En) => "Restore the Windows defaults",
        (RestoreSystem, Ru) => "Вернуть стандартные настройки Windows",
        (RestoreSystem, Uk) => "Повернути стандартні налаштування Windows",
        (ReplaceQuestion, En) => {
            "Open Snapt instead of the Snipping Tool on PrtSc and Win+Shift+S?\n\nWindows will then show its default apps page: pick Snapt for the screenshot links there. You can switch back any time in Snapt's settings."
        }
        (ReplaceQuestion, Ru) => {
            "Открывать Snapt вместо «Ножниц» по PrtSc и Win+Shift+S?\n\nЗатем Windows покажет страницу приложений по умолчанию: выберите там Snapt для ссылок на скриншоты. Вернуть «Ножницы» можно в любой момент в настройках Snapt."
        }
        (ReplaceQuestion, Uk) => {
            "Відкривати Snapt замість «Ножиць» за PrtSc і Win+Shift+S?\n\nПотім Windows покаже сторінку програм за замовчуванням: виберіть там Snapt для посилань на знімки. Повернути «Ножиці» можна будь-коли в налаштуваннях Snapt."
        }
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
        (CaptureCursor, En) => "Show the mouse pointer",
        (CaptureCursor, Ru) => "Показывать указатель мыши",
        (CaptureCursor, Uk) => "Показувати вказівник миші",
        (HintCaptureCursor, En) => "Draws the pointer on screenshots. Not on Wayland.",
        (HintCaptureCursor, Ru) => "Рисует указатель на скриншотах. На Wayland недоступно.",
        (HintCaptureCursor, Uk) => "Малює вказівник на знімках. На Wayland недоступно.",
        (Updates, En) => "Updates",
        (Updates, Ru) => "Обновления",
        (Updates, Uk) => "Оновлення",
        (Example, En) => "Example: {}",
        (Example, Ru) => "Пример: {}",
        (Example, Uk) => "Приклад: {}",
        (SecCapture, En) => "Capture",
        (SecCapture, Ru) => "Захват",
        (SecCapture, Uk) => "Захоплення",
        (SecSaving, En) => "Copy & save",
        (SecSaving, Ru) => "Сохранение",
        (SecSaving, Uk) => "Збереження",
        (SecSystem, En) => "System",
        (SecSystem, Ru) => "Система",
        (SecSystem, Uk) => "Система",
        (SecAbout, En) => "About",
        (SecAbout, Ru) => "О программе",
        (SecAbout, Uk) => "Про програму",
        (SubGeneral, En) => "Language, look and start-up.",
        (SubGeneral, Ru) => "Язык, внешний вид и запуск.",
        (SubGeneral, Uk) => "Мова, вигляд і запуск.",
        (SubCapture, En) => "What happens when you select an area.",
        (SubCapture, Ru) => "Что происходит, когда вы выделяете область.",
        (SubCapture, Uk) => "Що відбувається, коли ви виділяєте область.",
        (SubSaving, En) => "Where shots go and how they are copied.",
        (SubSaving, Ru) => "Куда попадают скриншоты и как они копируются.",
        (SubSaving, Uk) => "Куди потрапляють знімки і як вони копіюються.",
        (SubFeatures, En) => "Extra actions on the editor toolbar.",
        (SubFeatures, Ru) => "Дополнительные действия на панели редактора.",
        (SubFeatures, Uk) => "Додаткові дії на панелі редактора.",
        (SubSystem, En) => "Who answers PrtSc and the system screenshot keys.",
        (SubSystem, Ru) => "Кто отвечает на PrtSc и системные клавиши скриншотов.",
        (SubSystem, Uk) => "Хто відповідає на PrtSc і системні клавіші знімків.",
        (Theme, En) => "Theme",
        (Theme, Ru) => "Тема",
        (Theme, Uk) => "Тема",
        (ThemeSystem, En) => "System",
        (ThemeSystem, Ru) => "Системная",
        (ThemeSystem, Uk) => "Системна",
        (ThemeLight, En) => "Light",
        (ThemeLight, Ru) => "Светлая",
        (ThemeLight, Uk) => "Світла",
        (ThemeDark, En) => "Dark",
        (ThemeDark, Ru) => "Тёмная",
        (ThemeDark, Uk) => "Темна",
        (HintColorFormat, En) => "How the color picker copies a color.",
        (HintColorFormat, Ru) => "Как пипетка копирует цвет.",
        (HintColorFormat, Uk) => "Як піпетка копіює колір.",
        (HintCopyOnSelect, En) => {
            "No editor: the area goes to the clipboard as soon as you let go."
        }
        (HintCopyOnSelect, Ru) => {
            "Без редактора: область копируется, как только вы отпустите мышь."
        }
        (HintCopyOnSelect, Uk) => "Без редактора: область копіюється, щойно ви відпустите мишу.",
        (HintWindowOpensEditor, En) => "Works while copying right after selecting is on.",
        (HintWindowOpensEditor, Ru) => {
            "Работает, когда включено копирование сразу после выделения."
        }
        (HintWindowOpensEditor, Uk) => "Працює, коли ввімкнено копіювання одразу після виділення.",
        (HintCopyAs, En) => "A file can be pasted into a folder or a chat as a file.",
        (HintCopyAs, Ru) => "Файл можно вставить в папку или в чат как файл.",
        (HintCopyAs, Uk) => "Файл можна вставити в теку або в чат як файл.",
        (HintQuickSave, En) => "Saving goes straight into the folder, without a dialog.",
        (HintQuickSave, Ru) => "Сохранение сразу в папку, без окна выбора.",
        (HintQuickSave, Uk) => "Збереження одразу в теку, без вікна вибору.",
        (HintSaveOnCopy, En) => "Copied shots land in the folder too.",
        (HintSaveOnCopy, Ru) => "Скопированные скриншоты тоже попадают в папку.",
        (HintSaveOnCopy, Uk) => "Скопійовані знімки теж потрапляють у теку.",
        (FileName, En) => "File name",
        (FileName, Ru) => "Имя файла",
        (FileName, Uk) => "Ім'я файлу",
        (HintFileName, En) => {
            "%Y year, %m month, %d day, %H hour, %M minute, %S second. Enter applies."
        }
        (HintFileName, Ru) => {
            "%Y год, %m месяц, %d день, %H час, %M минута, %S секунда. Enter применяет."
        }
        (HintFileName, Uk) => {
            "%Y рік, %m місяць, %d день, %H година, %M хвилина, %S секунда. Enter застосовує."
        }
        (HistoryLimit, En) => "Recent shots in the tray",
        (HistoryLimit, Ru) => "Последние скриншоты в трее",
        (HistoryLimit, Uk) => "Останні знімки в треї",
        (Off, En) => "Off",
        (Off, Ru) => "Выкл",
        (Off, Uk) => "Вимк",
        (GroupAnywhere, En) => "Anywhere",
        (GroupAnywhere, Ru) => "В любом месте",
        (GroupAnywhere, Uk) => "Будь-де",
        (GroupInEditor, En) => "In the editor",
        (GroupInEditor, Ru) => "В редакторе",
        (GroupInEditor, Uk) => "У редакторі",
        (ResetSection, En) => "Reset this section",
        (ResetSection, Ru) => "Сбросить раздел",
        (ResetSection, Uk) => "Скинути розділ",
        (ResetConfirm, En) => "Click again to reset",
        (ResetConfirm, Ru) => "Нажмите ещё раз для сброса",
        (ResetConfirm, Uk) => "Натисніть ще раз, щоб скинути",
        (SearchSettings, En) => "Search",
        (SearchSettings, Ru) => "Поиск",
        (SearchSettings, Uk) => "Пошук",
        (NothingFound, En) => "Nothing found.",
        (NothingFound, Ru) => "Ничего не найдено.",
        (NothingFound, Uk) => "Нічого не знайдено.",
        (Version, En) => "Version {}",
        (Version, Ru) => "Версия {}",
        (Version, Uk) => "Версія {}",
        (License, En) => "Free and open source, MIT license.",
        (License, Ru) => "Бесплатно и с открытым кодом, лицензия MIT.",
        (License, Uk) => "Безкоштовно й з відкритим кодом, ліцензія MIT.",
        (MoreFromAuthor, En) => "More from the author",
        (MoreFromAuthor, Ru) => "Другие проекты автора",
        (MoreFromAuthor, Uk) => "Інші проєкти автора",
        (LevelerAbout, En) => "Evens out the loudness of any video.",
        (LevelerAbout, Ru) => "Выравнивает громкость любого видео.",
        (LevelerAbout, Uk) => "Вирівнює гучність будь-якого відео.",
        (VideoGlowAbout, En) => "Soft light around any video, like a strip behind a TV.",
        (VideoGlowAbout, Ru) => "Подсветка вокруг видео, как лента за телевизором.",
        (VideoGlowAbout, Uk) => "Підсвітка навколо відео, як стрічка за телевізором.",
        (HintRecognize, En) => "Copies the text in the area.",
        (HintRecognize, Ru) => "Копирует текст из области.",
        (HintRecognize, Uk) => "Копіює текст з області.",
        (HintScroll, En) => "Captures a long page by scrolling.",
        (HintScroll, Ru) => "Снимает длинную страницу с прокруткой.",
        (HintScroll, Uk) => "Знімає довгу сторінку з прокручуванням.",
        (HintRecord, En) => "Records the area as GIF or MP4.",
        (HintRecord, Ru) => "Записывает область в GIF или MP4.",
        (HintRecord, Uk) => "Записує область у GIF або MP4.",
        (HintPin, En) => "Keeps the shot on top of other windows.",
        (HintPin, Ru) => "Закрепляет скриншот поверх окон.",
        (HintPin, Uk) => "Закріплює знімок поверх вікон.",
        (HintSearch, En) => "Looks for similar images on the web.",
        (HintSearch, Ru) => "Ищет похожие картинки в интернете.",
        (HintSearch, Uk) => "Шукає схожі зображення в інтернеті.",
        (HintPrint, En) => "Prints the shot.",
        (HintPrint, Ru) => "Печатает скриншот.",
        (HintPrint, Uk) => "Друкує знімок.",
        (HintPicker, En) => "Picks a color anywhere on the screen.",
        (HintPicker, Ru) => "Берёт цвет с любого места экрана.",
        (HintPicker, Uk) => "Бере колір з будь-якого місця екрана.",

        (FullScreen, En) => "Copy full screen",
        (FullScreen, Ru) => "Скопировать весь экран",
        (FullScreen, Uk) => "Скопіювати весь екран",
        (RepeatArea, En) => "Repeat last area",
        (RepeatArea, Ru) => "Повторить прошлую область",
        (RepeatArea, Uk) => "Повторити минулу область",
        (PinLast, En) => "Pin last screenshot",
        (PinLast, Ru) => "Закрепить последний снимок",
        (PinLast, Uk) => "Закріпити останній знімок",
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
        (Scroll, En) => "Scrolling screenshot",
        (Scroll, Ru) => "Скриншот с прокруткой",
        (Scroll, Uk) => "Знімок із прокручуванням",
        (ScrollHint, En) => "Scroll down. Enter or click: done",
        (ScrollHint, Ru) => "Прокручивайте вниз. Enter или клик: готово",
        (ScrollHint, Uk) => "Прокручуйте вниз. Enter або клік: готово",
        (ScrollHeight, En) => "{} px so far · Esc cancels",
        (ScrollHeight, Ru) => "Уже {} px · Esc отменяет",
        (ScrollHeight, Uk) => "Уже {} px · Esc скасовує",
        (Record, En) => "Record video",
        (Record, Ru) => "Записать видео",
        (Record, Uk) => "Записати відео",
        (RecordHint, En) => "Recording. Enter or click: stop",
        (RecordHint, Ru) => "Идёт запись. Enter или клик: стоп",
        (RecordHint, Uk) => "Іде запис. Enter або клік: стоп",
        (RecordTime, En) => "{} · Esc cancels",
        (RecordTime, Ru) => "{} · Esc отменяет",
        (RecordTime, Uk) => "{} · Esc скасовує",
        (RecordTitle, En) => "Save recording",
        (RecordTitle, Ru) => "Сохранить запись",
        (RecordTitle, Uk) => "Зберегти запис",
        (GifVideo, En) => "GIF animation",
        (GifVideo, Ru) => "GIF-анимация",
        (GifVideo, Uk) => "GIF-анімація",
        (Mp4Video, En) => "MP4 video",
        (Mp4Video, Ru) => "Видео MP4",
        (Mp4Video, Uk) => "Відео MP4",
        (ReadQr, En) => "Read QR code",
        (ReadQr, Ru) => "Прочитать QR-код",
        (ReadQr, Uk) => "Прочитати QR-код",
        (NoQr, En) => "No QR code found in the selection.",
        (NoQr, Ru) => "В выделенной области нет QR-кода.",
        (NoQr, Uk) => "У виділеній області немає QR-коду.",
        (HidePersonal, En) => "Hide personal data",
        (HidePersonal, Ru) => "Скрыть личные данные",
        (HidePersonal, Uk) => "Приховати особисті дані",
        (Redacting, En) => "Looking for e-mails, phones and card numbers…",
        (Redacting, Ru) => "Ищем почты, телефоны и номера карт…",
        (Redacting, Uk) => "Шукаємо пошти, телефони й номери карток…",
        (NothingToHide, En) => "No e-mails, phones or card numbers found.",
        (NothingToHide, Ru) => "Почт, телефонов и номеров карт не найдено.",
        (NothingToHide, Uk) => "Пошт, телефонів і номерів карток не знайдено.",
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
                T::ReadQr,
                T::NoQr,
                T::HidePersonal,
                T::Redacting,
                T::NothingToHide,
            ] {
                for c in tr(lang, key).chars() {
                    assert!(crate::font::has_glyph(c), "{c:?} in {key:?} ({lang:?})");
                }
            }
        }
    }
}
