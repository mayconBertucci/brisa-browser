#![allow(
    non_snake_case,
    non_camel_case_types,
    reason = "Keep the WebView2 COM ABI names"
)]

use std::cell::{Cell, RefCell};

use windows_core::implement_decl;

use crate::{EventRegistration, WebView, bindings::*};
use windows_core::*;

const MAX_FAVICON: usize = 64 * 1024;

impl WebView {
    /// Observes favicon changes from the renderer's own navigation.
    pub fn on_favicon_changed(
        &self,
        callback: impl FnMut(WebView) + 'static,
    ) -> Result<EventRegistration> {
        let view: ICoreWebView2_15 = self.0.cast()?;
        let handler: ICoreWebView2FaviconChangedEventHandler =
            Changed(RefCell::new(Box::new(callback))).into();
        let mut token = 0;
        // SAFETY: The interface supports this vtable, the handler is a COM object,
        // and WebView2 records the registration in the writable token for this call.
        unsafe { (view.vtable().add_FaviconChanged)(view.as_raw(), handler.as_raw(), &mut token) }
            .ok()?;
        Ok(EventRegistration::new(move || {
            // SAFETY: This token came from the same live WebView2 interface.
            let _ = unsafe { (view.vtable().remove_FaviconChanged)(view.as_raw(), token) };
        }))
    }

    /// Returns the current icon as PNG data. An empty result indicates no icon.
    pub fn favicon_png(&self, callback: impl FnOnce(Result<Vec<u8>>) + 'static) -> Result<()> {
        let view: ICoreWebView2_15 = self.0.cast()?;
        let handler: ICoreWebView2GetFaviconCompletedHandler =
            Completed(Cell::new(Some(Box::new(callback)))).into();
        // SAFETY: This interface handles the documented PNG format (0) and
        // keeps its own reference to the completion handler as needed.
        unsafe { (view.vtable().GetFavicon)(view.as_raw(), 0, handler.as_raw()) }.ok()
    }
}

struct Changed(RefCell<Box<dyn FnMut(WebView)>>);

implement_decl! {
    impl Changed as Changed_Impl: [ICoreWebView2FaviconChangedEventHandler]
}

impl ICoreWebView2FaviconChangedEventHandler_Impl for Changed_Impl {
    fn Invoke(&self, sender: Ref<ICoreWebView2>, _: Ref<IUnknown>) -> Result<()> {
        if let Ok(view) = sender.ok()
            && let Ok(mut handler) = self.0.try_borrow_mut()
        {
            handler(WebView(view.clone()));
        }
        Ok(())
    }
}

struct Completed(Cell<Option<Box<dyn FnOnce(Result<Vec<u8>>)>>>);

implement_decl! {
    impl Completed as Completed_Impl: [ICoreWebView2GetFaviconCompletedHandler]
}

impl ICoreWebView2GetFaviconCompletedHandler_Impl for Completed_Impl {
    fn Invoke(&self, status: HRESULT, stream: Ref<IStream>) -> Result<()> {
        if let Some(callback) = self.0.take() {
            callback(status.ok().and_then(|()| match stream.ok() {
                Ok(stream) => read_stream(stream),
                Err(_) => Ok(Vec::new()),
            }));
        }
        Ok(())
    }
}

fn read_stream(stream: &IStream) -> Result<Vec<u8>> {
    let mut png = Vec::new();
    let mut buffer = [0u8; 4096];
    loop {
        let mut received = 0;
        // SAFETY: The IStream exposes ISequentialStream's read ABI. Its
        // output buffer and byte count are writable for this synchronous call.
        unsafe {
            (stream.vtable().base__.Read)(
                stream.as_raw(),
                buffer.as_mut_ptr().cast(),
                buffer.len() as u32,
                &mut received,
            )
        }
        .ok()?;
        let received = received as usize;
        if received == 0 {
            return Ok(png);
        }
        if received > buffer.len() || png.len() + received > MAX_FAVICON {
            return Ok(Vec::new());
        }
        png.extend_from_slice(&buffer[..received]);
    }
}

windows_core::imp::define_interface!(
    ICoreWebView2FaviconChangedEventHandler,
    ICoreWebView2FaviconChangedEventHandler_Vtbl,
    0x2913da94_833d_4de0_8dca_900fc524a1a4
);
windows_core::imp::interface_hierarchy!(ICoreWebView2FaviconChangedEventHandler, IUnknown);
#[repr(C)]
pub struct ICoreWebView2FaviconChangedEventHandler_Vtbl {
    pub base__: IUnknown_Vtbl,
    pub Invoke: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> HRESULT,
}
pub trait ICoreWebView2FaviconChangedEventHandler_Impl: IUnknownImpl {
    fn Invoke(&self, sender: Ref<ICoreWebView2>, args: Ref<IUnknown>) -> Result<()>;
}
impl ICoreWebView2FaviconChangedEventHandler_Vtbl {
    pub const fn new<
        Identity: ICoreWebView2FaviconChangedEventHandler_Impl,
        const OFFSET: isize,
    >() -> Self {
        unsafe extern "system" fn Invoke<
            Identity: ICoreWebView2FaviconChangedEventHandler_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            sender: *mut core::ffi::c_void,
            args: *mut core::ffi::c_void,
        ) -> HRESULT {
            // SAFETY: The COM stub supplies the identity and its borrowed ABI arguments.
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ICoreWebView2FaviconChangedEventHandler_Impl::Invoke(
                    this,
                    core::mem::transmute_copy(&sender),
                    core::mem::transmute_copy(&args),
                )
                .into()
            }
        }
        Self {
            base__: IUnknown_Vtbl::new::<Identity, OFFSET>(),
            Invoke: Invoke::<Identity, OFFSET>,
        }
    }
    pub fn matches(iid: &GUID) -> bool {
        iid == &<ICoreWebView2FaviconChangedEventHandler as Interface>::IID
    }
}
impl RuntimeName for ICoreWebView2FaviconChangedEventHandler {}

windows_core::imp::define_interface!(
    ICoreWebView2GetFaviconCompletedHandler,
    ICoreWebView2GetFaviconCompletedHandler_Vtbl,
    0xa2508329_7da8_49d7_8c05_fa125e4aee8d
);
windows_core::imp::interface_hierarchy!(ICoreWebView2GetFaviconCompletedHandler, IUnknown);
#[repr(C)]
pub struct ICoreWebView2GetFaviconCompletedHandler_Vtbl {
    pub base__: IUnknown_Vtbl,
    pub Invoke: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        HRESULT,
        *mut core::ffi::c_void,
    ) -> HRESULT,
}
pub trait ICoreWebView2GetFaviconCompletedHandler_Impl: IUnknownImpl {
    fn Invoke(&self, status: HRESULT, stream: Ref<IStream>) -> Result<()>;
}
impl ICoreWebView2GetFaviconCompletedHandler_Vtbl {
    pub const fn new<
        Identity: ICoreWebView2GetFaviconCompletedHandler_Impl,
        const OFFSET: isize,
    >() -> Self {
        unsafe extern "system" fn Invoke<
            Identity: ICoreWebView2GetFaviconCompletedHandler_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            status: HRESULT,
            stream: *mut core::ffi::c_void,
        ) -> HRESULT {
            // SAFETY: The COM stub supplies the identity and its borrowed ABI arguments.
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ICoreWebView2GetFaviconCompletedHandler_Impl::Invoke(
                    this,
                    status,
                    core::mem::transmute_copy(&stream),
                )
                .into()
            }
        }
        Self {
            base__: IUnknown_Vtbl::new::<Identity, OFFSET>(),
            Invoke: Invoke::<Identity, OFFSET>,
        }
    }
    pub fn matches(iid: &GUID) -> bool {
        iid == &<ICoreWebView2GetFaviconCompletedHandler as Interface>::IID
    }
}
impl RuntimeName for ICoreWebView2GetFaviconCompletedHandler {}
