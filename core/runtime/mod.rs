// Copyright 2018-2023 the Deno authors. All rights reserved. MIT license.
pub(crate) mod bindings;
pub(crate) mod exception_state;
mod jsrealm;
mod jsruntime;
pub mod op_driver;
#[doc(hidden)]
pub mod ops;
pub mod ops_rust_to_v8;
mod setup;
mod snapshot;
pub mod stats;

// #[cfg(all(test, not(miri)))]
#[cfg(test)]
mod tests;

pub const V8_WRAPPER_TYPE_INDEX: i32 = 0;
pub const V8_WRAPPER_OBJECT_INDEX: i32 = 1;

pub(crate) use jsrealm::ContextState;
pub(crate) use jsrealm::JsRealm;
pub(crate) use jsrealm::OpDriverImpl;
pub use jsruntime::CompiledWasmModuleStore;
pub use jsruntime::CreateRealmOptions;
pub use jsruntime::CrossIsolateStore;
pub(crate) use jsruntime::InitMode;
pub use jsruntime::JsRuntime;
pub use jsruntime::JsRuntimeForSnapshot;
pub use jsruntime::JsRuntimeState;
pub use jsruntime::PollEventLoopOptions;
pub use jsruntime::RuntimeOptions;
pub use jsruntime::SharedArrayBufferStore;
#[cfg(test)]
pub(crate) use jsruntime::NO_OF_BUILTIN_MODULES;
pub use snapshot::create_snapshot;
pub use snapshot::get_js_files;
pub use snapshot::CreateSnapshotOptions;
pub use snapshot::CreateSnapshotOutput;
pub use snapshot::FilterFn;
pub use snapshot::Snapshot;
pub use snapshot::SnapshotBulkCompressingSerializer;
pub use snapshot::SnapshotData;
pub(crate) use snapshot::SnapshotDataId;
pub use snapshot::SnapshotFileSerializer;
pub use snapshot::SnapshotInMemorySerializer;
pub(crate) use snapshot::SnapshotLoadDataStore;
pub use snapshot::SnapshotSerializer;
pub(crate) use snapshot::SnapshotStoreDataStore;
pub(crate) use snapshot::SnapshottedData;

pub use bindings::script_origin;
