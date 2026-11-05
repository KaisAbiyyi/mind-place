use mind_place_archive::{
    validate_capture, validate_historical_capture, validate_live_capture, validate_stored_capture,
    Capture, Message,
};

fn base_chatgpt_capture() -> Capture {
    Capture {
        schema_version: 1,
        provider: "chatgpt".into(),
        conversation_id: "12345678-1234-4123-8123-123456789abc".into(),
        source_url: "https://chatgpt.com/c/12345678-1234-4123-8123-123456789abc".into(),
        title: "Test Chat".into(),
        conversation_created_at: None,
        conversation_updated_at: None,
        captured_at: "2026-10-15T03:00:00.000Z".into(),
        capture_method: "manual-dom".into(),
        completeness: "partial".into(),
        warnings: vec![],
        messages: vec![Message {
            id: "msg-1".into(),
            role: "user".into(),
            text: "Hello".into(),
            format: "plain".into(),
            authored_at: None,
            model: None,
            attachments: vec![],
        }],
    }
}

fn base_gemini_capture() -> Capture {
    Capture {
        schema_version: 1,
        provider: "gemini".into(),
        conversation_id: "c_1234567890ab".into(),
        source_url: "https://gemini.google.com/app/c_1234567890ab".into(),
        title: "Gemini Chat".into(),
        conversation_created_at: None,
        conversation_updated_at: None,
        captured_at: "2026-10-15T03:00:00.000Z".into(),
        capture_method: "manual-dom".into(),
        completeness: "partial".into(),
        warnings: vec![],
        messages: vec![Message {
            id: "msg-1".into(),
            role: "user".into(),
            text: "Hello Gemini".into(),
            format: "plain".into(),
            authored_at: None,
            model: None,
            attachments: vec![],
        }],
    }
}

#[test]
fn test_live_capture_accepts_valid_chatgpt_and_gemini_manual_dom() {
    let mut chatgpt = base_chatgpt_capture();
    assert!(validate_live_capture(&mut chatgpt).is_ok());

    let mut gemini = base_gemini_capture();
    assert!(validate_live_capture(&mut gemini).is_ok());
}

#[test]
fn test_live_capture_rejects_historical_and_generic_methods() {
    // 1. Takeout import cannot be saved via live capture
    let mut cap_takeout = base_gemini_capture();
    cap_takeout.capture_method = "takeout-import".into();
    let err = validate_live_capture(&mut cap_takeout).unwrap_err();
    assert!(err
        .to_string()
        .contains("Historical or generic methods not permitted on live capture"));

    // 2. Export import cannot be saved via live capture
    let mut cap_export = base_chatgpt_capture();
    cap_export.capture_method = "export-import".into();
    let err = validate_live_capture(&mut cap_export).unwrap_err();
    assert!(err
        .to_string()
        .contains("Historical or generic methods not permitted on live capture"));

    // 3. Legacy import cannot be saved via live capture
    let mut cap_legacy = base_chatgpt_capture();
    cap_legacy.capture_method = "legacy-import".into();
    let err = validate_live_capture(&mut cap_legacy).unwrap_err();
    assert!(err
        .to_string()
        .contains("Historical or generic methods not permitted on live capture"));

    // 4. Generic import cannot be saved via live capture
    let mut cap_generic = base_chatgpt_capture();
    cap_generic.provider = "generic".into();
    cap_generic.capture_method = "generic-import".into();
    let err = validate_live_capture(&mut cap_generic).unwrap_err();
    assert!(err
        .to_string()
        .contains("Historical or generic methods not permitted on live capture"));
}

#[test]
fn test_live_capture_rejects_complete_completeness() {
    let mut cap = base_chatgpt_capture();
    cap.completeness = "complete".into();
    let err = validate_live_capture(&mut cap).unwrap_err();
    assert!(err
        .to_string()
        .contains("A DOM read cannot prove complete conversation history"));
}

#[test]
fn test_historical_capture_accepts_authorized_gemini_takeout() {
    let mut cap = base_gemini_capture();
    cap.capture_method = "takeout-import".into();
    cap.completeness = "complete".into();
    assert!(validate_historical_capture(&mut cap, "gemini").is_ok());

    // Synthetic Takeout URI
    let mut cap_synth = base_gemini_capture();
    cap_synth.conversation_id = "takeout_conv_001".into();
    cap_synth.source_url = "mindplace-import://gemini-takeout/takeout_conv_001".into();
    cap_synth.capture_method = "takeout-import".into();
    cap_synth.completeness = "complete".into();
    assert!(validate_historical_capture(&mut cap_synth, "gemini").is_ok());

    // Canvas exact content hash URI
    let canvas_hash = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let mut cap_canvas = base_gemini_capture();
    cap_canvas.conversation_id = canvas_hash.into();
    cap_canvas.source_url = format!("mindplace-import://gemini-canvas/{canvas_hash}");
    cap_canvas.capture_method = "takeout-import".into();
    cap_canvas.completeness = "complete".into();
    assert!(validate_historical_capture(&mut cap_canvas, "gemini").is_ok());
}

#[test]
fn test_historical_capture_accepts_authorized_chatgpt_export() {
    let mut cap = base_chatgpt_capture();
    cap.capture_method = "export-import".into();
    cap.completeness = "complete".into();
    assert!(validate_historical_capture(&mut cap, "chatgpt").is_ok());

    // Synthetic ChatGPT export URI
    let uuid_str = "abcdef12-3456-7890-abcd-ef1234567890";
    let mut cap_synth = base_chatgpt_capture();
    cap_synth.conversation_id = uuid_str.into();
    cap_synth.source_url = format!("mindplace-import://chatgpt-export/{uuid_str}");
    cap_synth.capture_method = "export-import".into();
    cap_synth.completeness = "complete".into();
    assert!(validate_historical_capture(&mut cap_synth, "chatgpt").is_ok());
}

#[test]
fn test_historical_capture_rejects_live_methods_and_mismatched_providers() {
    let mut cap = base_gemini_capture();
    cap.capture_method = "manual-dom".into();
    let err = validate_historical_capture(&mut cap, "gemini").unwrap_err();
    assert!(err
        .to_string()
        .contains("Live capture methods not permitted on historical import"));

    let mut cap_export = base_chatgpt_capture();
    cap_export.capture_method = "export-import".into();
    let err = validate_historical_capture(&mut cap_export, "gemini").unwrap_err();
    assert!(err
        .to_string()
        .contains("Provider mismatch in historical import"));
}

#[test]
fn test_validate_capture_wrapper_routes_correctly() {
    let mut live = base_chatgpt_capture();
    assert!(validate_capture(&mut live).is_ok());

    let mut takeout = base_gemini_capture();
    takeout.capture_method = "takeout-import".into();
    takeout.completeness = "complete".into();
    assert!(validate_capture(&mut takeout).is_ok());
}

#[test]
fn test_stored_capture_reader_compatibility() {
    // Stored captures must be readable whether they originated from live, takeout, or export!
    let mut cap_live = base_chatgpt_capture();
    validate_live_capture(&mut cap_live).unwrap();
    assert!(validate_stored_capture(&cap_live).is_ok());

    let mut cap_takeout = base_gemini_capture();
    cap_takeout.capture_method = "takeout-import".into();
    cap_takeout.completeness = "complete".into();
    validate_historical_capture(&mut cap_takeout, "gemini").unwrap();
    assert!(validate_stored_capture(&cap_takeout).is_ok());

    let mut cap_export = base_chatgpt_capture();
    cap_export.capture_method = "export-import".into();
    cap_export.completeness = "complete".into();
    validate_historical_capture(&mut cap_export, "chatgpt").unwrap();
    assert!(validate_stored_capture(&cap_export).is_ok());
}
