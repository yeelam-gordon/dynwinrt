// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use core::ffi::c_void;
use std::ffi::CString;

use libffi::middle::{Arg, Cif, CodePtr, Type};
use windows::Win32::Foundation::{FreeLibrary, GetLastError, HMODULE, SetLastError};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows_core::{HRESULT, HSTRING, PCSTR};

use crate::{
    metadata_table::TypeKind,
    result::{Error, Result},
    value::WinRTValue,
};

struct LoadedLibrary(HMODULE);

impl LoadedLibrary {
    fn load(dll: &str) -> Result<Self> {
        unsafe { LoadLibraryW(&HSTRING::from(dll)) }
            .map(Self)
            .map_err(Error::WindowsError)
    }

    fn proc_address(&self, entry: &str) -> Result<*mut c_void> {
        let entry = CString::new(entry).map_err(|_| invalid_arg_error())?;
        let proc = unsafe { GetProcAddress(self.0, PCSTR::from_raw(entry.as_ptr().cast())) };
        match proc {
            Some(proc) => Ok(unsafe { std::mem::transmute(proc) }),
            None => Err(Error::WindowsError(windows_core::Error::from_thread())),
        }
    }
}

impl Drop for LoadedLibrary {
    fn drop(&mut self) {
        unsafe {
            let last_error = GetLastError();
            let _ = FreeLibrary(self.0);
            SetLastError(last_error);
        }
    }
}

/// Owns a NUL-terminated UTF-16 string for passing as a stable `LPCWSTR` argument.
pub struct WideStringArg {
    buffer: Vec<u16>,
}

impl WideStringArg {
    pub fn as_winrt_value(&self) -> WinRTValue {
        WinRTValue::RawPtr(self.buffer.as_ptr() as *mut c_void)
    }
}

pub fn wide_string_arg(value: &str) -> WideStringArg {
    let mut buffer: Vec<u16> = value.encode_utf16().collect();
    buffer.push(0);
    WideStringArg { buffer }
}

pub fn get_last_error() -> u32 {
    unsafe { GetLastError().0 }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlatReturnKind {
    I32,
    U32,
    Ptr,
}

pub fn flat_invoke(
    dll: &str,
    entry: &str,
    ret: FlatReturnKind,
    args: &[WinRTValue],
) -> Result<WinRTValue> {
    #[cfg(not(all(windows, target_pointer_width = "64")))]
    {
        let _ = (dll, entry, ret, args);
        return Err(unsupported_platform_error());
    }

    #[cfg(all(windows, target_pointer_width = "64"))]
    {
        let library = LoadedLibrary::load(dll)?;
        let proc = library.proc_address(entry)?;
        let arg_types = args
            .iter()
            .map(flat_arg_type)
            .collect::<Result<Vec<Type>>>()?;
        let ffi_args = args.iter().map(flat_arg).collect::<Result<Vec<Arg>>>()?;
        let ret_type = flat_return_type(ret)?;
        let cif = Cif::new(arg_types, ret_type);

        // On x64 Windows there is a single native calling convention, so libffi's
        // default ABI is correct for Winapi/stdcall and cdecl flat exports.
        unsafe { call_and_convert(&cif, proc, &ffi_args, ret) }
    }
}

fn flat_arg_type(value: &WinRTValue) -> Result<Type> {
    match value {
        WinRTValue::RawPtr(_) | WinRTValue::Null => Ok(Type::pointer()),
        WinRTValue::I32(_) => Ok(Type::i32()),
        WinRTValue::U32(_) => Ok(Type::u32()),
        _ => Err(Error::InvalidType(TypeKind::I32, value.get_type_kind())),
    }
}

fn flat_arg(value: &WinRTValue) -> Result<Arg<'_>> {
    match value {
        WinRTValue::I32(_) | WinRTValue::U32(_) | WinRTValue::RawPtr(_) | WinRTValue::Null => {
            Ok(value.libffi_arg())
        }
        _ => Err(Error::InvalidType(TypeKind::I32, value.get_type_kind())),
    }
}

fn flat_return_type(kind: FlatReturnKind) -> Result<Type> {
    match kind {
        FlatReturnKind::I32 => Ok(Type::i32()),
        FlatReturnKind::U32 => Ok(Type::u32()),
        FlatReturnKind::Ptr => Ok(Type::pointer()),
    }
}

unsafe fn call_and_convert(
    cif: &Cif,
    proc: *mut c_void,
    args: &[Arg<'_>],
    ret: FlatReturnKind,
) -> Result<WinRTValue> {
    match ret {
        FlatReturnKind::I32 => Ok(WinRTValue::I32(unsafe { cif.call(CodePtr(proc), args) })),
        FlatReturnKind::U32 => Ok(WinRTValue::U32(unsafe { cif.call(CodePtr(proc), args) })),
        FlatReturnKind::Ptr => Ok(WinRTValue::RawPtr(unsafe {
            cif.call::<*mut c_void>(CodePtr(proc), args)
        })),
    }
}

fn invalid_arg_error() -> Error {
    Error::WindowsError(windows_core::Error::from_hresult(HRESULT(
        0x80070057u32 as i32,
    )))
}

#[cfg(not(all(windows, target_pointer_width = "64")))]
fn unsupported_platform_error() -> Error {
    Error::WindowsError(windows_core::Error::from_hresult(HRESULT(
        0x80004001u32 as i32,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_call_mul_div_multiplies_divides_and_rounds() -> Result<()> {
        let result = flat_invoke(
            "kernel32.dll",
            "MulDiv",
            FlatReturnKind::I32,
            &[WinRTValue::I32(100), WinRTValue::I32(3), WinRTValue::I32(2)],
        )?;
        assert_eq!(result.as_i32(), Some(150));

        let rounded = flat_invoke(
            "kernel32.dll",
            "MulDiv",
            FlatReturnKind::I32,
            &[WinRTValue::I32(7), WinRTValue::I32(1), WinRTValue::I32(2)],
        )?;
        assert_eq!(rounded.as_i32(), Some(4));
        Ok(())
    }

    #[test]
    fn flat_call_get_current_process_id_matches_rust_process_id() -> Result<()> {
        let result = flat_invoke(
            "kernel32.dll",
            "GetCurrentProcessId",
            FlatReturnKind::U32,
            &[],
        )?;
        let WinRTValue::U32(pid) = result else {
            panic!("expected U32 process id");
        };
        assert_eq!(pid, std::process::id());
        Ok(())
    }

    #[test]
    fn flat_call_lstrlenw_accepts_wide_string_pointer() -> Result<()> {
        let hello = wide_string_arg("hello");
        let result = flat_invoke(
            "kernel32.dll",
            "lstrlenW",
            FlatReturnKind::I32,
            &[hello.as_winrt_value()],
        )?;
        assert_eq!(result.as_i32(), Some(5));

        let empty = wide_string_arg("");
        let result = flat_invoke(
            "kernel32.dll",
            "lstrlenW",
            FlatReturnKind::I32,
            &[empty.as_winrt_value()],
        )?;
        assert_eq!(result.as_i32(), Some(0));
        Ok(())
    }

    #[test]
    fn flat_call_nonexistent_dll_returns_error() {
        let result = flat_invoke("no_such_dll_xyz.dll", "MulDiv", FlatReturnKind::I32, &[]);
        assert!(result.is_err());
    }

    #[test]
    fn flat_call_nonexistent_export_returns_error() {
        let result = flat_invoke(
            "kernel32.dll",
            "ThisExportDoesNotExist",
            FlatReturnKind::I32,
            &[],
        );
        assert!(result.is_err());
    }

    #[test]
    fn flat_call_get_module_handlew_uses_get_last_error_model() -> Result<()> {
        let bogus_module = wide_string_arg("no_such_module_xyz.dll");
        let result = flat_invoke(
            "kernel32.dll",
            "GetModuleHandleW",
            FlatReturnKind::Ptr,
            &[bogus_module.as_winrt_value()],
        )?;
        let WinRTValue::RawPtr(module) = result else {
            panic!("expected raw pointer return");
        };
        assert!(module.is_null());
        assert_eq!(get_last_error(), 126);
        Ok(())
    }
}
