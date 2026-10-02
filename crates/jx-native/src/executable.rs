use crate::{Operation, SLOTS};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::default_libcall_names;
use std::sync::{Arc, Mutex};

type Entry = unsafe extern "C" fn(*const f64, *const u8, *mut f64, *mut u8) -> u8;
struct Memory(Option<JITModule>);
impl Drop for Memory {
    fn drop(&mut self) {
        if let Some(module) = self.0.take() {
            // SAFETY: this private owner frees memory on compilation failure or
            // after the final Arc drops. No borrowed call or pointer can survive it.
            unsafe { module.free_memory() };
        }
    }
}
struct Owner {
    // JITModule is Send but not Sync. This mutex owns immutable code; evaluation
    // never locks it. It preserves sharing without an unsafe trait implementation.
    _memory: Mutex<Memory>,
    entry: Entry,
}
#[derive(Clone)]
pub struct Kernel {
    owner: Arc<Owner>,
    code_bytes: usize,
}
impl std::fmt::Debug for Kernel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Kernel")
            .field("code_bytes", &self.code_bytes)
            .finish()
    }
}
impl Kernel {
    pub fn compile(operations: &[Operation], result: u8) -> Result<Self, String> {
        let builder = JITBuilder::with_flags(&[("opt_level", "speed")], default_libcall_names())
            .map_err(|e| e.to_string())?;
        let mut memory = Memory(Some(JITModule::new(builder)));
        let module = memory.0.as_mut().unwrap();
        let (id, code_bytes) = crate::compile::compile(module, operations, result)?;
        let pointer = module.get_finalized_function(id);
        // SAFETY: the private compiler emits precisely Entry's native C ABI,
        // bounded reads into SLOTS arrays, and two output writes on success.
        let entry = unsafe { std::mem::transmute::<*const u8, Entry>(pointer) };
        Ok(Self {
            owner: Arc::new(Owner {
                _memory: Mutex::new(memory),
                entry,
            }),
            code_bytes,
        })
    }
    pub fn code_bytes(&self) -> usize {
        self.code_bytes
    }
    pub fn run(&self, numbers: &[f64; SLOTS], tags: &[u8; SLOTS]) -> Option<(f64, u8)> {
        let mut number = 0.0;
        let mut tag = 0;
        // SAFETY: slices have the compiler's exact bound and outputs are writable,
        // non-overlapping locals. Borrowing self pins executable memory for the call.
        let success =
            unsafe { (self.owner.entry)(numbers.as_ptr(), tags.as_ptr(), &mut number, &mut tag) };
        (success == 1).then_some((number, tag))
    }
}
