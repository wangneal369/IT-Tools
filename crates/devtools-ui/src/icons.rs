//! On-demand Lucide SVG icon bundle (`icon_assets!` + default component `Assets`).
//!
//! Avoids pulling `gpui_kit::assets::AllAssets` (1800+ embedded SVGs) so release
//! binaries stay well beneath the 45 MiB budget while keeping crisp monochrome
//! vector icons across the shell and all 28 tools.

use std::borrow::Cow;
use std::sync::{Arc, OnceLock};

use devtools_core::{Category, ToolId};
pub use gpui_kit::assets::IconName;
use gpui_kit::assets::{Assets, icon_assets};
use gpui_kit::component::{Icon, Sizable, Size};
use gpui_kit::*;

const BRAND_ICON_PNG: &[u8] = include_bytes!("../../../assets/icons/devtools-64.png");

/// Cached full-color DevTools brand icon (64×64 PNG) for the sidebar mark.
pub fn brand_icon_image() -> Arc<Image> {
    static IMAGE: OnceLock<Arc<Image>> = OnceLock::new();
    IMAGE
        .get_or_init(|| Arc::new(Image::from_bytes(ImageFormat::Png, BRAND_ICON_PNG.to_vec())))
        .clone()
}

icon_assets!(
    SelectedIcons,
    [
        ArrowLeftRight,
        Binary,
        Braces,
        Bug,
        CalendarClock,
        Check,
        ChevronDown,
        ChevronRight,
        CircleAlert,
        CircleCheck,
        CircleX,
        ClipboardCheck,
        ClipboardPaste,
        Clock,
        Code,
        CodeXml,
        Columns2,
        Command,
        Copy,
        Database,
        Eraser,
        Eye,
        EyeOff,
        FileArchive,
        FileBadge,
        FileCheck,
        FileDiff,
        FileSearch,
        FingerprintPattern,
        FlaskConical,
        GitCompare,
        GitCompareArrows,
        Grid3x3,
        Hash,
        House,
        Info,
        KeyRound,
        Layers,
        LayoutGrid,
        Link2,
        ListChecks,
        Lock,
        Maximize2,
        Minus,
        Moon,
        Network,
        Palette,
        PanelLeftClose,
        PanelLeftOpen,
        Play,
        Plus,
        QrCode,
        Quote,
        RefreshCw,
        Regex,
        Rows2,
        ScanText,
        Search,
        Send,
        Settings,
        ShieldCheck,
        ShieldKeyhole,
        SlidersHorizontal,
        Square,
        SquareTerminal,
        Star,
        StarFill,
        Sun,
        Table2,
        Timer,
        WandSparkles,
        X,
        Zap,
    ]
);

/// Combined asset source serving our selected Lucide icons first, falling back
/// to `gpui_kit::assets::Assets` for internal component icons.
#[derive(Clone, Copy, Debug, Default)]
pub struct WorkbenchAssets;

impl AssetSource for WorkbenchAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path.is_empty() {
            return Ok(None);
        }
        if let Some(bytes) = SelectedIcons.load(path)? {
            return Ok(Some(bytes));
        }
        Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut items = SelectedIcons.list(path)?;
        for item in Assets.list(path)? {
            if !items.contains(&item) {
                items.push(item);
            }
        }
        Ok(items)
    }
}

/// Build a sized, colored monochrome vector icon element.
pub fn icon_el(name: IconName, size_px: f32, color: u32) -> Icon {
    Icon::new(name)
        .with_size(Size::Size(px(size_px)))
        .text_color(rgb(color))
}

/// Primary sidebar icon for each category.
pub const fn category_icon(category: Category) -> IconName {
    match category {
        Category::Conversion => IconName::ArrowLeftRight,
        Category::EncodeDecode => IconName::Binary,
        Category::Format => IconName::Braces,
        Category::Generator => IconName::WandSparkles,
        Category::Text => IconName::GitCompare,
        Category::Debug => IconName::Bug,
        Category::Security => IconName::ShieldCheck,
    }
}

/// Dedicated semantic icon for each of the 28 V1 tools.
pub const fn tool_icon(tool: ToolId) -> IconName {
    match tool {
        ToolId::DataFormatConverter => IconName::ArrowLeftRight,
        ToolId::DateTimeConverter => IconName::CalendarClock,
        ToolId::NumberBaseConverter => IconName::Hash,
        ToolId::ColorConverter => IconName::Palette,
        ToolId::JsonToTable => IconName::Table2,
        ToolId::Base64Codec => IconName::Binary,
        ToolId::UrlCodec => IconName::Link2,
        ToolId::HtmlEntityUnicode => IconName::Code,
        ToolId::StringEscape => IconName::Quote,
        ToolId::GzipCompress => IconName::FileArchive,
        ToolId::JsonFormatter => IconName::Braces,
        ToolId::SqlFormatter => IconName::Database,
        ToolId::XmlHtmlFormatter => IconName::CodeXml,
        ToolId::UuidUlidGenerator => IconName::FingerprintPattern,
        ToolId::PasswordTokenGenerator => IconName::KeyRound,
        ToolId::QrCodeGenerator => IconName::QrCode,
        ToolId::TextDiff => IconName::FileDiff,
        ToolId::JsonDiff => IconName::GitCompareArrows,
        ToolId::TextAnalysis => IconName::ScanText,
        ToolId::ListCompare => IconName::ListChecks,
        ToolId::RegexTester => IconName::Regex,
        ToolId::JsonPathTester => IconName::FileSearch,
        ToolId::CronParser => IconName::Timer,
        ToolId::ChmodCalculator => IconName::Grid3x3,
        ToolId::JwtTool => IconName::ShieldKeyhole,
        ToolId::HashHmac => IconName::FileCheck,
        ToolId::X509Parser => IconName::FileBadge,
        ToolId::Ipv4CidrCalculator => IconName::Network,
    }
}

#[cfg(test)]
mod tests {
    use super::{IconName, WorkbenchAssets, category_icon, tool_icon};
    use devtools_core::{Category, ToolId};
    use gpui_kit::AssetSource;

    #[test]
    fn every_shell_category_and_tool_icon_loads_from_bundled_assets() {
        let assets = WorkbenchAssets;
        for category in Category::ALL {
            let path = category_icon(category).path();
            assert!(
                assets.load(path.as_ref()).expect("load icon").is_some(),
                "missing category icon: {path}"
            );
        }
        for tool in ToolId::ALL {
            let path = tool_icon(tool).path();
            assert!(
                assets.load(path.as_ref()).expect("load icon").is_some(),
                "missing tool icon: {path}"
            );
        }
        for ui_icon in [
            IconName::House,
            IconName::Star,
            IconName::StarFill,
            IconName::Clock,
            IconName::Search,
            IconName::Settings,
            IconName::PanelLeftClose,
            IconName::PanelLeftOpen,
            IconName::SquareTerminal,
            IconName::LayoutGrid,
            IconName::Rows2,
            IconName::Columns2,
            IconName::Square,
            IconName::Copy,
            IconName::ClipboardCheck,
            IconName::ClipboardPaste,
            IconName::Send,
            IconName::Play,
            IconName::CircleX,
            IconName::CircleAlert,
            IconName::CircleCheck,
            IconName::FlaskConical,
            IconName::Eraser,
            IconName::RefreshCw,
            IconName::Lock,
            IconName::Plus,
            IconName::Minus,
            IconName::X,
            IconName::Zap,
        ] {
            let path = ui_icon.path();
            assert!(
                assets.load(path.as_ref()).expect("load icon").is_some(),
                "missing shell icon: {path}"
            );
        }
    }
}
