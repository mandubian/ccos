use crate::runtime::{Evaluator, Value, RuntimeError};

pub struct VM<'a> {
    chunk: Chunk,
    ip: usize,
    stack: Vec<Value>,
    env: Rc<RefCell<Environment>>,
    evaluator: &'a mut Evaluator,
}

impl<'a> VM<'a> {
    pub fn new(chunk: Chunk, evaluator: &'a mut Evaluator) -> Self {
        VM {
            chunk,
            ip: 0,
            stack: Vec::with_capacity(256),
            env: evaluator.get_env(),
            evaluator,
        }
    }

    pub fn run(&mut self) -> Result<Value, String> {
        loop {
            let instruction = self.fetch_byte()?;
            match instruction {
                OpCode::Constant => {
                    let constant = self.fetch_constant()?;
                    self.push(constant)?;
                }
                OpCode::Add => {
                    self.binary_op(|a, b| a + b)?;
                }
                OpCode::Subtract => {
                    self.binary_op(|a, b| a - b)?;
                }
                OpCode::Multiply => {
                    self.binary_op(|a, b| a * b)?;
                }
                OpCode::Divide => {
                    self.binary_op(|a, b| a / b)?;
                }
                OpCode::Negate => {
                    let value = self.pop()?;
                    self.push(-value)?;
                }
                OpCode::Return => {
                    let result = self.pop()?;
                    return Ok(result);
                }
                OpCode::Call => {
                    let arg_count = self.fetch_byte()? as usize;
                    let callee = self.stack[self.stack.len() - 1 - arg_count..][0].clone();
                    match callee {
                        Value::Closure(closure) => {
                            if closure.params.len() != arg_count {
                                return Err(format!(
                                    "Expected {} arguments but got {}",
                                    closure.params.len(),
                                    arg_count
                                ));
                            }
                            let frame = CallFrame {
                                closure,
                                ip: self.ip,
                                stack_start: self.stack.len() - arg_count - 1,
                            };
                            self.frames.push(frame);
                            self.ip = 0; // ip will be updated by frame's chunk
                        }
                        Value::NativeFn(native_fn) => {
                            let args = &self.stack[self.stack.len() - arg_count..];
                            let result = if let Some(func_with_eval) = native_fn.func_with_evaluator {
                                func_with_eval(args, &mut self.evaluator)
                            } else {
                                (native_fn.func)(args)
                            };

                            match result {
                                Ok(value) => {
                                    self.stack.truncate(self.stack.len() - arg_count - 1);
                                    self.stack.push(value);
                                }
                                Err(e) => return Err(format!("Error in native function: {:?}", e)),
                            }
                        }
                        _ => return Err("Callee is not a function".to_string()),
                    }
                }
                _ => unimplemented!("Opcode not implemented"),
            }
        }
    }

    // ...existing methods...
}