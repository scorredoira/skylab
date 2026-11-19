// A worker of the game (`engine::threads`): loads the module with the page's memory, then takes
// jobs for ever. The page's thread starts it and sends it the compiled module and the memory.
import init, { worker_entry } from "./flight.js";

self.onmessage = async ({ data: [module, memory] }) => {
  // 2 MB of stack, as a native thread has.
  await init({ module_or_path: module, memory, thread_stack_size: 2 << 20 });
  worker_entry();
};
