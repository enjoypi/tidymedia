
#[test]
fn pixel_multibyte_straddling_cut_returns_none() {
    assert!(matchers::try_pixel("PXL_20230615_1030中5", utc_offset()).is_none());
}

#[test]
fn pixel_multibyte_after_cut_still_parses() {
    let c = matchers::try_pixel("PXL_20230615_103045é1", utc_offset()).unwrap();
    assert_eq!(c.utc.timestamp(), epoch("2023-06-15T10:30:45Z"));
    assert_eq!(c.source, Source::FilenamePixel);
}

#[test]
fn screenshot_dash_multibyte_straddling_cut_returns_none() {
    assert!(matchers::try_screenshot("Screenshot_2024-05-17-12-00-0é12", utc_offset()).is_none());
}

#[test]
fn screenshot_compact_multibyte_straddling_cut_returns_none() {
    assert!(matchers::try_screenshot("Screenshot_20240517_1200中5", utc_offset()).is_none());
}

#[test]
fn screenshot_multibyte_after_cut_still_parses() {
    let c = matchers::try_screenshot("Screenshot_2024-05-17-12-00-00é", utc_offset()).unwrap();
    assert_eq!(c.utc.timestamp(), epoch("2024-05-17T12:00:00Z"));
    assert_eq!(c.source, Source::FilenameScreenshot);
}

#[test]
fn whatsapp_multibyte_straddling_cut_returns_none() {
    assert!(
        matchers::try_whatsapp("WhatsApp Image 2024-05-17 at 12.00.0é12", utc_offset())
            .is_none()
    );
}

#[test]
fn whatsapp_multibyte_after_cut_still_parses() {
    let c = matchers::try_whatsapp("WhatsApp Image 2024-05-17 at 12.00.00é", utc_offset()).unwrap();
    assert_eq!(c.utc.timestamp(), epoch("2024-05-17T12:00:00Z"));
    assert_eq!(c.source, Source::FilenameWhatsApp);
}

#[test]
fn macos_screenshot_multibyte_straddling_cut_returns_none() {
    assert!(
        matchers::try_macos_screenshot("Screen Shot 2024-05-17 at 12.00.0é12", utc_offset())
            .is_none()
    );
}

#[test]
fn macos_screenshot_multibyte_after_cut_still_parses() {
    let c =
        matchers::try_macos_screenshot("Screen Shot 2024-05-17 at 12.00.00é", utc_offset())
            .unwrap();
    assert_eq!(c.utc.timestamp(), epoch("2024-05-17T12:00:00Z"));
    assert_eq!(c.source, Source::FilenameScreenshot);
}
