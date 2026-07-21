// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! TDD tests for the *Interop HWND pattern in classic-COM code generation.
//!
//! These tests drive the `getForWindow(hwnd, REFIID, out void**)` special case:
//! - IUnknown-rooted interop (e.g. `IDataTransferManagerInterop`, base=+3)
//! - IInspectable-rooted interop (e.g. `ISystemMediaTransportControlsInterop`, base=+6)
//!
//! The interop shape is: last two params are `(riid: In, out_ptr: Out)`, plus
//! zero or more natural in-params (HWND, HSTRING, …). The generated wrapper
//! MUST hide the REFIID + void** — the caller only supplies the natural
//! parameters, and the wrapper returns the projected WinRT object.
//!
//! Skipped when the Win32 winmd is not present at the well-known path.

use std::fs;
use std::path::{Path, PathBuf};

use dynwinrt_codegen::codegen::com;
use dynwinrt_codegen::meta;

const WIN32_WINMD: &str = r"C:\s\win32metadata\Windows.Win32.winmd";
const WINDOWS_WINMD: &str =
    r"C:\Program Files (x86)\Windows Kits\10\UnionMetadata\10.0.26100.0\Windows.winmd";

fn win32_available() -> bool {
    Path::new(WIN32_WINMD).exists()
}

fn windows_winmd_available() -> bool {
    Path::new(WINDOWS_WINMD).exists()
}

/// 1. IDataTransferManagerInterop parses cleanly, is IUnknown-rooted (+3),
///    and its `GetForWindow` is at slot 3.
#[test]
fn parse_data_transfer_manager_interop() {
    if !win32_available() {
        eprintln!("Skipping: Win32 winmd not available");
        return;
    }
    let com = meta::parse_com_interface(
        WIN32_WINMD,
        "Windows.Win32.UI.Shell",
        "IDataTransferManagerInterop",
    )
    .expect("IDataTransferManagerInterop must exist");
    assert!(com.is_iunknown_rooted);
    assert_eq!(com.base_offset, 3);
    let get_for_window = com
        .interface
        .methods
        .iter()
        .find(|m| m.name == "GetForWindow")
        .expect("GetForWindow method must exist");
    assert_eq!(get_for_window.vtable_index, 3);
    // Last two params must be (In riid, Out out_ptr) — the interop shape.
    assert_eq!(get_for_window.params.len(), 3, "HWND + riid + out");
}

/// 2. ISystemMediaTransportControlsInterop parses cleanly, is IInspectable-rooted (+6),
///    `GetForWindow` at slot 6.
#[test]
fn parse_smtc_interop() {
    if !win32_available() {
        eprintln!("Skipping: Win32 winmd not available");
        return;
    }
    let com = meta::parse_com_interface(
        WIN32_WINMD,
        "Windows.Win32.System.WinRT",
        "ISystemMediaTransportControlsInterop",
    )
    .expect("ISystemMediaTransportControlsInterop must exist");
    assert!(!com.is_iunknown_rooted, "SMTC interop derives from IInspectable, not IUnknown");
    assert_eq!(com.base_offset, 6);
    let get_for_window = com
        .interface
        .methods
        .iter()
        .find(|m| m.name == "GetForWindow")
        .expect("GetForWindow method must exist");
    assert_eq!(get_for_window.vtable_index, 6);
}

/// 3. Codegen recognises the interop shape and emits a natural
///    `getForWindow(hwnd)` — hiding both the REFIID and the void** out-ptr.
#[test]
fn interop_dts_hides_riid_and_out_ptr_for_datatransfermanager() {
    if !win32_available() || !windows_winmd_available() {
        eprintln!("Skipping: winmd(s) not available");
        return;
    }
    let com = meta::parse_com_interface(
        WIN32_WINMD,
        "Windows.Win32.UI.Shell",
        "IDataTransferManagerInterop",
    )
    .unwrap();
    let out = com::generate_com_interface_files(&com, WIN32_WINMD);
    let dts = out.dts.as_str();

    // The natural signature: hwnd only, NO riid, NO out-ptr.
    // Accept either single-arg or single-arg + optional projection hint.
    // The signature must contain `getForWindow(` followed by a SINGLE
    // typed parameter (HWND-like) and NO `riid`/`REFIID` mention.
    assert!(
        dts.contains("getForWindow"),
        ".d.ts must expose getForWindow (camelCased):\n{}",
        dts
    );
    assert!(
        !dts.contains("riid") && !dts.contains("REFIID"),
        "REFIID/riid must not appear in .d.ts:\n{}",
        dts
    );
    assert!(
        !dts.contains("void**") && !dts.to_lowercase().contains("out_ptr"),
        "void**/out_ptr must not appear in .d.ts:\n{}",
        dts
    );

    // Return type — must be a NATURAL WinRT type name, not `bigint | Buffer`
    // and not the raw `unknown` fallback.
    // For IDataTransferManagerInterop → DataTransferManager.
    assert!(
        dts.contains("DataTransferManager"),
        ".d.ts must project the return type as DataTransferManager:\n{}",
        dts
    );
    assert!(
        !dts.contains("getForWindow(hwnd: bigint | Buffer, riid"),
        "riid must not leak into the natural signature:\n{}",
        dts
    );
}

/// 4. The generated JS synthesises the target IID (default interface IID of
///    the WinRT runtime class) INSIDE the method body — the caller supplies
///    only the HWND.
#[test]
fn interop_js_synthesizes_target_iid_for_datatransfermanager() {
    if !win32_available() || !windows_winmd_available() {
        eprintln!("Skipping: winmd(s) not available");
        return;
    }
    let com = meta::parse_com_interface(
        WIN32_WINMD,
        "Windows.Win32.UI.Shell",
        "IDataTransferManagerInterop",
    )
    .unwrap();
    let out = com::generate_com_interface_files(&com, WIN32_WINMD);
    let js = out.js.as_str();

    // The IDataTransferManager default interface IID must be embedded in .js
    // (it's the runtime class's default interface's IID:
    // a5caee9b-8708-49d1-8d36-67d25a8da00c).
    assert!(
        js.contains("a5caee9b-8708-49d1-8d36-67d25a8da00c"),
        ".js must embed the IDataTransferManager default interface IID:\n{}",
        js
    );
    // The interop's own IID must also be present.
    assert!(
        js.contains("3a3dcd6c-3eab-43dc-bcde-45671ce800c8"),
        ".js must embed the IDataTransferManagerInterop IID:\n{}",
        js
    );

    // GetForWindow lives at vtable slot 3 (IUnknown+3).
    assert!(
        js.contains("method(3)"),
        ".js must invoke slot 3 for GetForWindow:\n{}",
        js
    );

    // Activation: uses activationFactory (WinRT) for the projected class
    // + QI to the interop IID — NOT CoCreateInstance (which is for classic COM CLSIDs).
    assert!(
        js.contains("activationFactory") || js.contains("activation_factory"),
        ".js must use activationFactory to reach the interop:\n{}",
        js
    );
    assert!(
        !js.contains("coCreateInstance"),
        "interop must NOT use coCreateInstance (only WinRT interop path):\n{}",
        js
    );
}

/// 5. SMTC-specific: the SMTC interop generates a wrapper whose registration
///    uses the +6 (IInspectable) base, and its GetForWindow invokes slot 6.
#[test]
fn smtc_interop_js_uses_inspectable_base_slot_6() {
    if !win32_available() || !windows_winmd_available() {
        eprintln!("Skipping: winmd(s) not available");
        return;
    }
    let com = meta::parse_com_interface(
        WIN32_WINMD,
        "Windows.Win32.System.WinRT",
        "ISystemMediaTransportControlsInterop",
    )
    .unwrap();
    let out = com::generate_com_interface_files(&com, WIN32_WINMD);
    let js = out.js.as_str();

    // IInspectable-rooted → register with the WinRT base (registerInterface),
    // not registerInterfaceUnknown.
    assert!(
        js.contains("registerInterface(") && !js.contains("registerInterfaceUnknown("),
        ".js for an IInspectable-rooted interop must use registerInterface \
         (base_slot=6), got:\n{}",
        js
    );
    assert!(
        js.contains("method(6)"),
        ".js must invoke slot 6 for GetForWindow:\n{}",
        js
    );

    // Return type = SystemMediaTransportControls; default interface IID
    // (ISystemMediaTransportControls = 99fa3ff4-1742-42a6-902e-087d41f965ec).
    assert!(
        js.contains("99fa3ff4-1742-42a6-902e-087d41f965ec"),
        ".js must embed the ISystemMediaTransportControls default interface IID:\n{}",
        js
    );
}

/// 6. The interop wrapper's return object exposes `runtimeClassName` — a
///    natural, meaningful property that reads via IInspectable::GetRuntimeClassName.
///    This is what the E2E asserts to prove the returned object is a live WinRT
///    object (not just a non-null pointer).
#[test]
fn interop_return_type_exposes_runtime_class_name() {
    if !win32_available() || !windows_winmd_available() {
        eprintln!("Skipping: winmd(s) not available");
        return;
    }
    let com = meta::parse_com_interface(
        WIN32_WINMD,
        "Windows.Win32.UI.Shell",
        "IDataTransferManagerInterop",
    )
    .unwrap();
    let out = com::generate_com_interface_files(&com, WIN32_WINMD);

    // The projected class `DataTransferManager` is emitted as a separate
    // sibling file (own .js + .d.ts), NOT inside the interop wrapper's .d.ts.
    let projected_dts = out
        .extra_files
        .iter()
        .find(|(name, _)| name == "DataTransferManager.d.ts")
        .map(|(_, content)| content.as_str())
        .expect(
            "DataTransferManager.d.ts must be emitted as a projected companion \
             (via Windows.winmd default-interface lookup)",
        );

    assert!(
        projected_dts.contains("runtimeClassName"),
        "DataTransferManager.d.ts must declare a `runtimeClassName` getter:\n{}",
        projected_dts
    );
    assert!(
        projected_dts.contains("class DataTransferManager"),
        "DataTransferManager.d.ts must declare `class DataTransferManager`:\n{}",
        projected_dts
    );
    assert!(
        projected_dts.contains("static getForWindow"),
        "DataTransferManager.d.ts must expose `static getForWindow(hwnd)`:\n{}",
        projected_dts
    );

    // Also confirm the interop's own .d.ts references DataTransferManager as
    // the natural return type (verified via import).
    assert!(
        out.dts.contains("DataTransferManager"),
        "interop .d.ts must reference the projected return type:\n{}",
        out.dts
    );
}

/// 7. Interop generation is deterministic (byte-identical across two runs).
#[test]
fn interop_generation_is_deterministic() {
    if !win32_available() || !windows_winmd_available() {
        eprintln!("Skipping: winmd(s) not available");
        return;
    }
    let mk = || {
        let com = meta::parse_com_interface(
            WIN32_WINMD,
            "Windows.Win32.UI.Shell",
            "IDataTransferManagerInterop",
        )
        .unwrap();
        com::generate_com_interface_files(&com, WIN32_WINMD)
    };
    let a = mk();
    let b = mk();
    assert_eq!(a.js, b.js);
    assert_eq!(a.dts, b.dts);
    assert_eq!(a.extra_files, b.extra_files);
}

/// 8. Snapshot: lock the generated IDataTransferManagerInterop files
///    against committed reference files.
#[test]
fn snapshot_datatransfermanager_interop() {
    if !win32_available() || !windows_winmd_available() {
        eprintln!("Skipping: winmd(s) not available");
        return;
    }
    let com = meta::parse_com_interface(
        WIN32_WINMD,
        "Windows.Win32.UI.Shell",
        "IDataTransferManagerInterop",
    )
    .unwrap();
    let out = com::generate_com_interface_files(&com, WIN32_WINMD);

    let snapshot_dir: PathBuf =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots/idatatransfermanagerinterop");
    assert!(
        snapshot_dir.exists(),
        "Snapshot directory not found: {}",
        snapshot_dir.display()
    );

    let mut generated: Vec<(String, String)> = Vec::new();
    generated.push(("IDataTransferManagerInterop.js".into(), out.js.clone()));
    generated.push(("IDataTransferManagerInterop.d.ts".into(), out.dts.clone()));
    for (name, content) in &out.extra_files {
        generated.push((name.clone(), content.clone()));
    }

    let mut mismatches = Vec::new();
    for (name, actual) in &generated {
        let path = snapshot_dir.join(name);
        if !path.exists() {
            mismatches.push(format!("  missing snapshot: {}", name));
            continue;
        }
        let expected = fs::read_to_string(&path).unwrap();
        if actual.trim_end() != expected.trim_end() {
            mismatches.push(format!("  differs: {}", name));
        }
    }
    if let Ok(entries) = fs::read_dir(&snapshot_dir) {
        let names: std::collections::HashSet<String> =
            generated.iter().map(|(n, _)| n.clone()).collect();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !names.contains(&name) {
                mismatches.push(format!("  extra snapshot not generated: {}", name));
            }
        }
    }

    if !mismatches.is_empty() {
        panic!(
            "IDataTransferManagerInterop snapshot mismatch!\n{}\n\n\
             To update, re-run the generator or copy the actual output.",
            mismatches.join("\n")
        );
    }
}
