// Generated by `wit-bindgen` 0.36.0. DO NOT EDIT!
// Options used:
//   * runtime_path: "wit_bindgen_rt"
#[rustfmt::skip]
#[allow(dead_code, clippy::all)]
pub mod exports {
    pub mod paxos {
        pub mod acceptor {
            #[allow(dead_code, clippy::all)]
            pub mod types {
                #[used]
                #[doc(hidden)]
                static __FORCE_SECTION_REF: fn() = super::super::super::super::__link_custom_section_describing_imports;
                use super::super::super::super::_rt;
                /// record proposer {
                /// id: u32,
                /// crnd: u32,
                /// nummodes: u32,
                /// // promises: list<tuple<u32, promise>>,
                /// // values: list<tuple<u32, value>>,
                /// // receivedpromises: list<u32>
                /// }
                #[derive(Clone)]
                pub struct Promise {
                    pub to: u32,
                    pub from: u32,
                    pub rnd: u32,
                    pub vrnd: u32,
                    pub vval: _rt::String,
                }
                impl ::core::fmt::Debug for Promise {
                    fn fmt(
                        &self,
                        f: &mut ::core::fmt::Formatter<'_>,
                    ) -> ::core::fmt::Result {
                        f.debug_struct("Promise")
                            .field("to", &self.to)
                            .field("from", &self.from)
                            .field("rnd", &self.rnd)
                            .field("vrnd", &self.vrnd)
                            .field("vval", &self.vval)
                            .finish()
                    }
                }
                #[derive(Clone)]
                pub struct Accept {
                    pub from: u32,
                    pub rnd: u32,
                    pub val: _rt::String,
                }
                impl ::core::fmt::Debug for Accept {
                    fn fmt(
                        &self,
                        f: &mut ::core::fmt::Formatter<'_>,
                    ) -> ::core::fmt::Result {
                        f.debug_struct("Accept")
                            .field("from", &self.from)
                            .field("rnd", &self.rnd)
                            .field("val", &self.val)
                            .finish()
                    }
                }
                #[repr(C)]
                #[derive(Clone, Copy)]
                pub struct Prepare {
                    pub from: u32,
                    pub crnd: u32,
                }
                impl ::core::fmt::Debug for Prepare {
                    fn fmt(
                        &self,
                        f: &mut ::core::fmt::Formatter<'_>,
                    ) -> ::core::fmt::Result {
                        f.debug_struct("Prepare")
                            .field("from", &self.from)
                            .field("crnd", &self.crnd)
                            .finish()
                    }
                }
                #[derive(Clone)]
                pub struct Learn {
                    pub from: u32,
                    pub rnd: u32,
                    pub val: _rt::String,
                }
                impl ::core::fmt::Debug for Learn {
                    fn fmt(
                        &self,
                        f: &mut ::core::fmt::Formatter<'_>,
                    ) -> ::core::fmt::Result {
                        f.debug_struct("Learn")
                            .field("from", &self.from)
                            .field("rnd", &self.rnd)
                            .field("val", &self.val)
                            .finish()
                    }
                }
                #[doc(hidden)]
                macro_rules! __export_paxos_acceptor_types_0_1_0_cabi {
                    ($ty:ident with_types_in $($path_to_types:tt)*) => {
                        const _ : () = {};
                    };
                }
                #[doc(hidden)]
                pub(crate) use __export_paxos_acceptor_types_0_1_0_cabi;
            }
            #[allow(dead_code, clippy::all)]
            pub mod acceptorinterface {
                #[used]
                #[doc(hidden)]
                static __FORCE_SECTION_REF: fn() = super::super::super::super::__link_custom_section_describing_imports;
                use super::super::super::super::_rt;
                pub type Promise = super::super::super::super::exports::paxos::acceptor::types::Promise;
                pub type Accept = super::super::super::super::exports::paxos::acceptor::types::Accept;
                pub type Prepare = super::super::super::super::exports::paxos::acceptor::types::Prepare;
                pub type Learn = super::super::super::super::exports::paxos::acceptor::types::Learn;
                #[derive(Debug)]
                #[repr(transparent)]
                pub struct Acceptorresource {
                    handle: _rt::Resource<Acceptorresource>,
                }
                type _AcceptorresourceRep<T> = Option<T>;
                impl Acceptorresource {
                    /// Creates a new resource from the specified representation.
                    ///
                    /// This function will create a new resource handle by moving `val` onto
                    /// the heap and then passing that heap pointer to the component model to
                    /// create a handle. The owned handle is then returned as `Acceptorresource`.
                    pub fn new<T: GuestAcceptorresource>(val: T) -> Self {
                        Self::type_guard::<T>();
                        let val: _AcceptorresourceRep<T> = Some(val);
                        let ptr: *mut _AcceptorresourceRep<T> = _rt::Box::into_raw(
                            _rt::Box::new(val),
                        );
                        unsafe { Self::from_handle(T::_resource_new(ptr.cast())) }
                    }
                    /// Gets access to the underlying `T` which represents this resource.
                    pub fn get<T: GuestAcceptorresource>(&self) -> &T {
                        let ptr = unsafe { &*self.as_ptr::<T>() };
                        ptr.as_ref().unwrap()
                    }
                    /// Gets mutable access to the underlying `T` which represents this
                    /// resource.
                    pub fn get_mut<T: GuestAcceptorresource>(&mut self) -> &mut T {
                        let ptr = unsafe { &mut *self.as_ptr::<T>() };
                        ptr.as_mut().unwrap()
                    }
                    /// Consumes this resource and returns the underlying `T`.
                    pub fn into_inner<T: GuestAcceptorresource>(self) -> T {
                        let ptr = unsafe { &mut *self.as_ptr::<T>() };
                        ptr.take().unwrap()
                    }
                    #[doc(hidden)]
                    pub unsafe fn from_handle(handle: u32) -> Self {
                        Self {
                            handle: _rt::Resource::from_handle(handle),
                        }
                    }
                    #[doc(hidden)]
                    pub fn take_handle(&self) -> u32 {
                        _rt::Resource::take_handle(&self.handle)
                    }
                    #[doc(hidden)]
                    pub fn handle(&self) -> u32 {
                        _rt::Resource::handle(&self.handle)
                    }
                    #[doc(hidden)]
                    fn type_guard<T: 'static>() {
                        use core::any::TypeId;
                        static mut LAST_TYPE: Option<TypeId> = None;
                        unsafe {
                            assert!(! cfg!(target_feature = "atomics"));
                            let id = TypeId::of::<T>();
                            match LAST_TYPE {
                                Some(ty) => {
                                    assert!(
                                        ty == id, "cannot use two types with this resource type"
                                    )
                                }
                                None => LAST_TYPE = Some(id),
                            }
                        }
                    }
                    #[doc(hidden)]
                    pub unsafe fn dtor<T: 'static>(handle: *mut u8) {
                        Self::type_guard::<T>();
                        let _ = _rt::Box::from_raw(
                            handle as *mut _AcceptorresourceRep<T>,
                        );
                    }
                    fn as_ptr<T: GuestAcceptorresource>(
                        &self,
                    ) -> *mut _AcceptorresourceRep<T> {
                        Acceptorresource::type_guard::<T>();
                        T::_resource_rep(self.handle()).cast()
                    }
                }
                /// A borrowed version of [`Acceptorresource`] which represents a borrowed value
                /// with the lifetime `'a`.
                #[derive(Debug)]
                #[repr(transparent)]
                pub struct AcceptorresourceBorrow<'a> {
                    rep: *mut u8,
                    _marker: core::marker::PhantomData<&'a Acceptorresource>,
                }
                impl<'a> AcceptorresourceBorrow<'a> {
                    #[doc(hidden)]
                    pub unsafe fn lift(rep: usize) -> Self {
                        Self {
                            rep: rep as *mut u8,
                            _marker: core::marker::PhantomData,
                        }
                    }
                    /// Gets access to the underlying `T` in this resource.
                    pub fn get<T: GuestAcceptorresource>(&self) -> &T {
                        let ptr = unsafe { &mut *self.as_ptr::<T>() };
                        ptr.as_ref().unwrap()
                    }
                    fn as_ptr<T: 'static>(&self) -> *mut _AcceptorresourceRep<T> {
                        Acceptorresource::type_guard::<T>();
                        self.rep.cast()
                    }
                }
                unsafe impl _rt::WasmResource for Acceptorresource {
                    #[inline]
                    unsafe fn drop(_handle: u32) {
                        #[cfg(not(target_arch = "wasm32"))]
                        unreachable!();
                        #[cfg(target_arch = "wasm32")]
                        {
                            #[link(
                                wasm_import_module = "[export]paxos:acceptor/acceptorinterface@0.1.0"
                            )]
                            extern "C" {
                                #[link_name = "[resource-drop]acceptorresource"]
                                fn drop(_: u32);
                            }
                            drop(_handle);
                        }
                    }
                }
                #[doc(hidden)]
                #[allow(non_snake_case)]
                pub unsafe fn _export_constructor_acceptorresource_cabi<
                    T: GuestAcceptorresource,
                >(arg0: i32) -> i32 {
                    #[cfg(target_arch = "wasm32")] _rt::run_ctors_once();
                    let result0 = Acceptorresource::new(T::new(arg0 as u32));
                    (result0).take_handle() as i32
                }
                #[doc(hidden)]
                #[allow(non_snake_case)]
                pub unsafe fn _export_method_acceptorresource_handleprepare_cabi<
                    T: GuestAcceptorresource,
                >(arg0: *mut u8, arg1: i32, arg2: i32) -> *mut u8 {
                    #[cfg(target_arch = "wasm32")] _rt::run_ctors_once();
                    let result0 = T::handleprepare(
                        AcceptorresourceBorrow::lift(arg0 as u32 as usize).get(),
                        super::super::super::super::exports::paxos::acceptor::types::Prepare {
                            from: arg1 as u32,
                            crnd: arg2 as u32,
                        },
                    );
                    let ptr1 = _RET_AREA.0.as_mut_ptr().cast::<u8>();
                    let super::super::super::super::exports::paxos::acceptor::types::Promise {
                        to: to2,
                        from: from2,
                        rnd: rnd2,
                        vrnd: vrnd2,
                        vval: vval2,
                    } = result0;
                    *ptr1.add(0).cast::<i32>() = _rt::as_i32(to2);
                    *ptr1.add(4).cast::<i32>() = _rt::as_i32(from2);
                    *ptr1.add(8).cast::<i32>() = _rt::as_i32(rnd2);
                    *ptr1.add(12).cast::<i32>() = _rt::as_i32(vrnd2);
                    let vec3 = (vval2.into_bytes()).into_boxed_slice();
                    let ptr3 = vec3.as_ptr().cast::<u8>();
                    let len3 = vec3.len();
                    ::core::mem::forget(vec3);
                    *ptr1.add(20).cast::<usize>() = len3;
                    *ptr1.add(16).cast::<*mut u8>() = ptr3.cast_mut();
                    ptr1
                }
                #[doc(hidden)]
                #[allow(non_snake_case)]
                pub unsafe fn __post_return_method_acceptorresource_handleprepare<
                    T: GuestAcceptorresource,
                >(arg0: *mut u8) {
                    let l0 = *arg0.add(16).cast::<*mut u8>();
                    let l1 = *arg0.add(20).cast::<usize>();
                    _rt::cabi_dealloc(l0, l1, 1);
                }
                #[doc(hidden)]
                #[allow(non_snake_case)]
                pub unsafe fn _export_method_acceptorresource_handleaccept_cabi<
                    T: GuestAcceptorresource,
                >(
                    arg0: *mut u8,
                    arg1: i32,
                    arg2: i32,
                    arg3: *mut u8,
                    arg4: usize,
                ) -> *mut u8 {
                    #[cfg(target_arch = "wasm32")] _rt::run_ctors_once();
                    let len0 = arg4;
                    let bytes0 = _rt::Vec::from_raw_parts(arg3.cast(), len0, len0);
                    let result1 = T::handleaccept(
                        AcceptorresourceBorrow::lift(arg0 as u32 as usize).get(),
                        super::super::super::super::exports::paxos::acceptor::types::Accept {
                            from: arg1 as u32,
                            rnd: arg2 as u32,
                            val: _rt::string_lift(bytes0),
                        },
                    );
                    let ptr2 = _RET_AREA.0.as_mut_ptr().cast::<u8>();
                    let super::super::super::super::exports::paxos::acceptor::types::Learn {
                        from: from3,
                        rnd: rnd3,
                        val: val3,
                    } = result1;
                    *ptr2.add(0).cast::<i32>() = _rt::as_i32(from3);
                    *ptr2.add(4).cast::<i32>() = _rt::as_i32(rnd3);
                    let vec4 = (val3.into_bytes()).into_boxed_slice();
                    let ptr4 = vec4.as_ptr().cast::<u8>();
                    let len4 = vec4.len();
                    ::core::mem::forget(vec4);
                    *ptr2.add(12).cast::<usize>() = len4;
                    *ptr2.add(8).cast::<*mut u8>() = ptr4.cast_mut();
                    ptr2
                }
                #[doc(hidden)]
                #[allow(non_snake_case)]
                pub unsafe fn __post_return_method_acceptorresource_handleaccept<
                    T: GuestAcceptorresource,
                >(arg0: *mut u8) {
                    let l0 = *arg0.add(8).cast::<*mut u8>();
                    let l1 = *arg0.add(12).cast::<usize>();
                    _rt::cabi_dealloc(l0, l1, 1);
                }
                pub trait Guest {
                    type Acceptorresource: GuestAcceptorresource;
                }
                pub trait GuestAcceptorresource: 'static {
                    #[doc(hidden)]
                    unsafe fn _resource_new(val: *mut u8) -> u32
                    where
                        Self: Sized,
                    {
                        #[cfg(not(target_arch = "wasm32"))]
                        {
                            let _ = val;
                            unreachable!();
                        }
                        #[cfg(target_arch = "wasm32")]
                        {
                            #[link(
                                wasm_import_module = "[export]paxos:acceptor/acceptorinterface@0.1.0"
                            )]
                            extern "C" {
                                #[link_name = "[resource-new]acceptorresource"]
                                fn new(_: *mut u8) -> u32;
                            }
                            new(val)
                        }
                    }
                    #[doc(hidden)]
                    fn _resource_rep(handle: u32) -> *mut u8
                    where
                        Self: Sized,
                    {
                        #[cfg(not(target_arch = "wasm32"))]
                        {
                            let _ = handle;
                            unreachable!();
                        }
                        #[cfg(target_arch = "wasm32")]
                        {
                            #[link(
                                wasm_import_module = "[export]paxos:acceptor/acceptorinterface@0.1.0"
                            )]
                            extern "C" {
                                #[link_name = "[resource-rep]acceptorresource"]
                                fn rep(_: u32) -> *mut u8;
                            }
                            unsafe { rep(handle) }
                        }
                    }
                    fn new(id: u32) -> Self;
                    fn handleprepare(&self, prepare: Prepare) -> Promise;
                    fn handleaccept(&self, accept: Accept) -> Learn;
                }
                #[doc(hidden)]
                macro_rules! __export_paxos_acceptor_acceptorinterface_0_1_0_cabi {
                    ($ty:ident with_types_in $($path_to_types:tt)*) => {
                        const _ : () = { #[export_name =
                        "paxos:acceptor/acceptorinterface@0.1.0#[constructor]acceptorresource"]
                        unsafe extern "C" fn export_constructor_acceptorresource(arg0 :
                        i32,) -> i32 { $($path_to_types)*::
                        _export_constructor_acceptorresource_cabi::<<$ty as
                        $($path_to_types)*:: Guest >::Acceptorresource > (arg0) }
                        #[export_name =
                        "paxos:acceptor/acceptorinterface@0.1.0#[method]acceptorresource.handleprepare"]
                        unsafe extern "C" fn
                        export_method_acceptorresource_handleprepare(arg0 : * mut u8,
                        arg1 : i32, arg2 : i32,) -> * mut u8 { $($path_to_types)*::
                        _export_method_acceptorresource_handleprepare_cabi::<<$ty as
                        $($path_to_types)*:: Guest >::Acceptorresource > (arg0, arg1,
                        arg2) } #[export_name =
                        "cabi_post_paxos:acceptor/acceptorinterface@0.1.0#[method]acceptorresource.handleprepare"]
                        unsafe extern "C" fn
                        _post_return_method_acceptorresource_handleprepare(arg0 : * mut
                        u8,) { $($path_to_types)*::
                        __post_return_method_acceptorresource_handleprepare::<<$ty as
                        $($path_to_types)*:: Guest >::Acceptorresource > (arg0) }
                        #[export_name =
                        "paxos:acceptor/acceptorinterface@0.1.0#[method]acceptorresource.handleaccept"]
                        unsafe extern "C" fn
                        export_method_acceptorresource_handleaccept(arg0 : * mut u8, arg1
                        : i32, arg2 : i32, arg3 : * mut u8, arg4 : usize,) -> * mut u8 {
                        $($path_to_types)*::
                        _export_method_acceptorresource_handleaccept_cabi::<<$ty as
                        $($path_to_types)*:: Guest >::Acceptorresource > (arg0, arg1,
                        arg2, arg3, arg4) } #[export_name =
                        "cabi_post_paxos:acceptor/acceptorinterface@0.1.0#[method]acceptorresource.handleaccept"]
                        unsafe extern "C" fn
                        _post_return_method_acceptorresource_handleaccept(arg0 : * mut
                        u8,) { $($path_to_types)*::
                        __post_return_method_acceptorresource_handleaccept::<<$ty as
                        $($path_to_types)*:: Guest >::Acceptorresource > (arg0) } const _
                        : () = { #[doc(hidden)] #[export_name =
                        "paxos:acceptor/acceptorinterface@0.1.0#[dtor]acceptorresource"]
                        #[allow(non_snake_case)] unsafe extern "C" fn dtor(rep : * mut
                        u8) { $($path_to_types)*:: Acceptorresource::dtor::< <$ty as
                        $($path_to_types)*:: Guest >::Acceptorresource > (rep) } }; };
                    };
                }
                #[doc(hidden)]
                pub(crate) use __export_paxos_acceptor_acceptorinterface_0_1_0_cabi;
                #[repr(align(4))]
                struct _RetArea([::core::mem::MaybeUninit<u8>; 24]);
                static mut _RET_AREA: _RetArea = _RetArea(
                    [::core::mem::MaybeUninit::uninit(); 24],
                );
            }
        }
    }
}
#[rustfmt::skip]
mod _rt {
    pub use alloc_crate::string::String;
    use core::fmt;
    use core::marker;
    use core::sync::atomic::{AtomicU32, Ordering::Relaxed};
    /// A type which represents a component model resource, either imported or
    /// exported into this component.
    ///
    /// This is a low-level wrapper which handles the lifetime of the resource
    /// (namely this has a destructor). The `T` provided defines the component model
    /// intrinsics that this wrapper uses.
    ///
    /// One of the chief purposes of this type is to provide `Deref` implementations
    /// to access the underlying data when it is owned.
    ///
    /// This type is primarily used in generated code for exported and imported
    /// resources.
    #[repr(transparent)]
    pub struct Resource<T: WasmResource> {
        handle: AtomicU32,
        _marker: marker::PhantomData<T>,
    }
    /// A trait which all wasm resources implement, namely providing the ability to
    /// drop a resource.
    ///
    /// This generally is implemented by generated code, not user-facing code.
    #[allow(clippy::missing_safety_doc)]
    pub unsafe trait WasmResource {
        /// Invokes the `[resource-drop]...` intrinsic.
        unsafe fn drop(handle: u32);
    }
    impl<T: WasmResource> Resource<T> {
        #[doc(hidden)]
        pub unsafe fn from_handle(handle: u32) -> Self {
            debug_assert!(handle != u32::MAX);
            Self {
                handle: AtomicU32::new(handle),
                _marker: marker::PhantomData,
            }
        }
        /// Takes ownership of the handle owned by `resource`.
        ///
        /// Note that this ideally would be `into_handle` taking `Resource<T>` by
        /// ownership. The code generator does not enable that in all situations,
        /// unfortunately, so this is provided instead.
        ///
        /// Also note that `take_handle` is in theory only ever called on values
        /// owned by a generated function. For example a generated function might
        /// take `Resource<T>` as an argument but then call `take_handle` on a
        /// reference to that argument. In that sense the dynamic nature of
        /// `take_handle` should only be exposed internally to generated code, not
        /// to user code.
        #[doc(hidden)]
        pub fn take_handle(resource: &Resource<T>) -> u32 {
            resource.handle.swap(u32::MAX, Relaxed)
        }
        #[doc(hidden)]
        pub fn handle(resource: &Resource<T>) -> u32 {
            resource.handle.load(Relaxed)
        }
    }
    impl<T: WasmResource> fmt::Debug for Resource<T> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("Resource").field("handle", &self.handle).finish()
        }
    }
    impl<T: WasmResource> Drop for Resource<T> {
        fn drop(&mut self) {
            unsafe {
                match self.handle.load(Relaxed) {
                    u32::MAX => {}
                    other => T::drop(other),
                }
            }
        }
    }
    pub use alloc_crate::boxed::Box;
    #[cfg(target_arch = "wasm32")]
    pub fn run_ctors_once() {
        wit_bindgen_rt::run_ctors_once();
    }
    pub fn as_i32<T: AsI32>(t: T) -> i32 {
        t.as_i32()
    }
    pub trait AsI32 {
        fn as_i32(self) -> i32;
    }
    impl<'a, T: Copy + AsI32> AsI32 for &'a T {
        fn as_i32(self) -> i32 {
            (*self).as_i32()
        }
    }
    impl AsI32 for i32 {
        #[inline]
        fn as_i32(self) -> i32 {
            self as i32
        }
    }
    impl AsI32 for u32 {
        #[inline]
        fn as_i32(self) -> i32 {
            self as i32
        }
    }
    impl AsI32 for i16 {
        #[inline]
        fn as_i32(self) -> i32 {
            self as i32
        }
    }
    impl AsI32 for u16 {
        #[inline]
        fn as_i32(self) -> i32 {
            self as i32
        }
    }
    impl AsI32 for i8 {
        #[inline]
        fn as_i32(self) -> i32 {
            self as i32
        }
    }
    impl AsI32 for u8 {
        #[inline]
        fn as_i32(self) -> i32 {
            self as i32
        }
    }
    impl AsI32 for char {
        #[inline]
        fn as_i32(self) -> i32 {
            self as i32
        }
    }
    impl AsI32 for usize {
        #[inline]
        fn as_i32(self) -> i32 {
            self as i32
        }
    }
    pub unsafe fn cabi_dealloc(ptr: *mut u8, size: usize, align: usize) {
        if size == 0 {
            return;
        }
        let layout = alloc::Layout::from_size_align_unchecked(size, align);
        alloc::dealloc(ptr, layout);
    }
    pub use alloc_crate::vec::Vec;
    pub unsafe fn string_lift(bytes: Vec<u8>) -> String {
        if cfg!(debug_assertions) {
            String::from_utf8(bytes).unwrap()
        } else {
            String::from_utf8_unchecked(bytes)
        }
    }
    extern crate alloc as alloc_crate;
    pub use alloc_crate::alloc;
}
/// Generates `#[no_mangle]` functions to export the specified type as the
/// root implementation of all generated traits.
///
/// For more information see the documentation of `wit_bindgen::generate!`.
///
/// ```rust
/// # macro_rules! export{ ($($t:tt)*) => (); }
/// # trait Guest {}
/// struct MyType;
///
/// impl Guest for MyType {
///     // ...
/// }
///
/// export!(MyType);
/// ```
#[allow(unused_macros)]
#[doc(hidden)]
macro_rules! __export_acceptorworld_impl {
    ($ty:ident) => {
        self::export!($ty with_types_in self);
    };
    ($ty:ident with_types_in $($path_to_types_root:tt)*) => {
        $($path_to_types_root)*::
        exports::paxos::acceptor::types::__export_paxos_acceptor_types_0_1_0_cabi!($ty
        with_types_in $($path_to_types_root)*:: exports::paxos::acceptor::types);
        $($path_to_types_root)*::
        exports::paxos::acceptor::acceptorinterface::__export_paxos_acceptor_acceptorinterface_0_1_0_cabi!($ty
        with_types_in $($path_to_types_root)*::
        exports::paxos::acceptor::acceptorinterface);
    };
}
#[doc(inline)]
pub(crate) use __export_acceptorworld_impl as export;
#[cfg(target_arch = "wasm32")]
#[link_section = "component-type:wit-bindgen:0.36.0:paxos:acceptor@0.1.0:acceptorworld:encoded world"]
#[doc(hidden)]
pub static __WIT_BINDGEN_COMPONENT_TYPE: [u8; 697] = *b"\
\0asm\x0d\0\x01\0\0\x19\x16wit-component-encoding\x04\0\x07\xb5\x04\x01A\x02\x01\
A\x08\x01B\x08\x01r\x05\x02toy\x04fromy\x03rndy\x04vrndy\x04vvals\x04\0\x07promi\
se\x03\0\0\x01r\x03\x04fromy\x03rndy\x03vals\x04\0\x06accept\x03\0\x02\x01r\x02\x04\
fromy\x04crndy\x04\0\x07prepare\x03\0\x04\x01r\x03\x04fromy\x03rndy\x03vals\x04\0\
\x05learn\x03\0\x06\x04\0\x1apaxos:acceptor/types@0.1.0\x05\0\x02\x03\0\0\x07pro\
mise\x02\x03\0\0\x06accept\x02\x03\0\0\x07prepare\x02\x03\0\0\x05learn\x01B\x11\x02\
\x03\x02\x01\x01\x04\0\x07promise\x03\0\0\x02\x03\x02\x01\x02\x04\0\x06accept\x03\
\0\x02\x02\x03\x02\x01\x03\x04\0\x07prepare\x03\0\x04\x02\x03\x02\x01\x04\x04\0\x05\
learn\x03\0\x06\x04\0\x10acceptorresource\x03\x01\x01i\x08\x01@\x01\x02idy\0\x09\
\x04\0\x1d[constructor]acceptorresource\x01\x0a\x01h\x08\x01@\x02\x04self\x0b\x07\
prepare\x05\0\x01\x04\0&[method]acceptorresource.handleprepare\x01\x0c\x01@\x02\x04\
self\x0b\x06accept\x03\0\x07\x04\0%[method]acceptorresource.handleaccept\x01\x0d\
\x04\0&paxos:acceptor/acceptorinterface@0.1.0\x05\x05\x04\0\"paxos:acceptor/acce\
ptorworld@0.1.0\x04\0\x0b\x13\x01\0\x0dacceptorworld\x03\0\0\0G\x09producers\x01\
\x0cprocessed-by\x02\x0dwit-component\x070.220.0\x10wit-bindgen-rust\x060.36.0";
#[inline(never)]
#[doc(hidden)]
pub fn __link_custom_section_describing_imports() {
    wit_bindgen_rt::maybe_link_cabi_realloc();
}
