use crate::busmap::BusMap;
use crate::error::{ErrorKind, N2VError};
use crate::parser::*;
use crate::scanner::Scanner;
use crate::simulator::{Bus, Chip, Port, Simulator};
use crate::test_parser::*;
/// For dealing with nand2tetris tests
use crate::test_scanner::TestScanner;
use bitvec::prelude::*;
use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::io::{prelude::*, BufReader};
use std::path::PathBuf;
use std::ptr;
use std::rc::Rc;

fn test_input_to_bitvec(input: &InputValue) -> BitVec<u16, Msb0> {
    match input.number_system {
        NumberSystem::Decimal => {
            let num: i16 = input.value.parse().unwrap();
            let mut raw = [0u16; 1];
            raw.view_bits_mut::<Msb0>().store_le(num);
            let bits = raw.view_bits::<Msb0>();
            bits.to_bitvec()
        }
        NumberSystem::Binary => {
            let bools: Vec<bool> = input
                .value
                .chars()
                .map(|c| match c {
                    '0' => false,
                    '1' => true,
                    _ => {
                        panic!("expected 0 or 1");
                    }
                })
                .collect();
            BitVec::from_iter(bools)
        }
        NumberSystem::Hex => {
            panic!("hex number system not supported yet.");
        }
        NumberSystem::String => {
            // this isn't a bit vector - what to do?
            panic!("string number system not supported yet.");
        }
    }
}

fn bitvec_to_vecbool(bv: BitVec<u16, Msb0>) -> Vec<Option<bool>> {
    let mut res = Vec::new();
    for bit in bv {
        res.push(Some(bit));
    }
    res
}

/// Reads a nand2tetris cmp file and returns a busmap of values
fn read_cmp(
    path: &PathBuf,
    test_script: &TestScript,
    ports: &HashMap<String, Port>,
) -> Result<Vec<BusMap>, N2VError> {
    let mut res: Vec<BusMap> = Vec::new();
    let file = fs::File::open(path).unwrap_or_else(|_| panic!("No such cmp file {:?}", path));
    let buf = BufReader::new(file);

    let mut lines = buf.lines();

    // Read header line and determine order of ports
    let mut header = lines.next().unwrap().expect("Corrupted cmp file");
    header.retain(|c| !c.is_whitespace());

    // We need at least three characters for a valid header line:
    // two pipes and a single letter port name.
    if header.len() < 3 {
        return Err(N2VError {
            msg: format!("Header line for cmp file {:?} is too short. The header line is the first line of the .cmp file.", path),
            kind: ErrorKind::Other,
        });
    }

    let port_order: Vec<String> = header[1..header.len() - 1]
        .split('|')
        .map(|p| p.to_string())
        .collect();

    while let Some(Ok(l)) = lines.next() {
        if l.is_empty() {
            continue;
        }
        let mut step_result = BusMap::new();
        let mut line = l.clone();
        line.retain(|c| !c.is_whitespace());

        if line.len() < 3 {
            return Err(N2VError {
                msg: format!(
                    "The line {} in {:?} is too short to be correct.",
                    line, path
                ),
                kind: ErrorKind::Other,
            });
        }

        for (i, v) in line[1..line.len() - 1].split('|').enumerate() {
            if i >= test_script.output_list.len() {
                return Err(N2VError {
                    msg: format!(
                        "The line {} in {:?} contains more columns than the test script output-list.", line, path
                    ),
                    kind: ErrorKind::Other,
                });
            }
            let number_system = test_script.output_list[i].number_system.clone();
            if number_system == NumberSystem::String {
                continue;
            }

            // Ignore wildcard expected output.
            if v.contains('*') {
                continue;
            }

            let bitvec_value = test_input_to_bitvec(&InputValue {
                number_system: number_system.clone(),
                value: v.to_string(),
            });

            let mut value = bitvec_to_vecbool(bitvec_value);
            value.reverse();

            if i >= port_order.len() {
                return Err(N2VError {
                    msg: format!(
                        "The line {} in {:?} contains more columns than the header line.",
                        line, path
                    ),
                    kind: ErrorKind::Other,
                });
            }

            let portw = match ports.get(&port_order[i]) {
                None => {
                    return Err(N2VError {
                        msg: format!("CMP / HDL mismatch. The .cmp file refers to port `{}`, but the HDL file does not.", port_order[i]),
                        kind: ErrorKind::Other,
                    });
                }
                Some(x) => x,
            };

            value.truncate(portw.width);
            value.reverse();
            step_result.create_bus(&port_order[i], value.len()).unwrap();
            let bus = Bus {
                name: port_order[i].clone(),
                range: Some(0..value.len()),
            };
            step_result.insert_option(&bus, value);
        }
        res.push(step_result);
    }

    Ok(res)
}

pub fn run_test(test_script_path: &str) -> Result<(), Box<dyn Error>> {
    // Parse the test script
    let test_pathbuf = PathBuf::from(test_script_path);
    let test_contents = read_test(&test_pathbuf)?;
    let mut test_scanner = TestScanner::new(test_contents.as_str(), test_pathbuf.clone());
    let mut test_parser = TestParser {
        scanner: &mut test_scanner,
    };
    let test_script = test_parser.parse().expect("Parse failure");
    let hdl_path = test_pathbuf.parent().unwrap().join(&test_script.hdl_file);

    // Create simulator for HDL file referenced by test script.
    let base_path = hdl_path.parent().unwrap().to_str().unwrap();
    let hdl_file = hdl_path.file_name().unwrap().to_str().unwrap();
    let provider: Rc<dyn HdlProvider> = Rc::new(FileReader::new(base_path));
    let contents = provider.get_hdl(hdl_file).unwrap();
    let mut scanner = Scanner::new(contents.as_str(), provider.get_path(hdl_file));
    let mut parser = Parser {
        scanner: &mut scanner,
    };
    let hdl = match parser.parse() {
        Ok(x) => x,
        Err(x) => {
            println!("{}", x);
            std::process::exit(1);
        }
    };

    let chip = match Chip::new(
        &hdl,
        ptr::null_mut(),
        &provider,
        false,
        &test_script.generics,
    ) {
        Ok(x) => x,
        Err(x) => {
            println!("{}", x);
            std::process::exit(1);
        }
    };

    let mut simulator = Simulator::new(chip);

    let hdl_contents = fs::read_to_string(hdl_path.clone()).expect("Unable to read HDL file.");
    let mut scanner = Scanner::new(hdl_contents.as_str(), hdl_path);
    let mut parser = Parser {
        scanner: &mut scanner,
    };
    let hdl = parser.parse().expect("Parse error");
    let chip = Chip::new(
        &hdl,
        ptr::null_mut(),
        &provider,
        false,
        &test_script.generics,
    )
    .expect("Chip creation error");

    let ports = chip.ports;
    let compare_path = test_pathbuf
        .parent()
        .unwrap()
        .join(&test_script.compare_file);
    let expected = read_cmp(&compare_path, &test_script, &ports)?;

    let mut inputs = BusMap::new();
    let mut cmp_idx = 0;
    let mut failures = 0;
    for step in &test_script.steps {
        let mut outputs = BusMap::new();
        for instruction in &step.instructions {
            match instruction {
                Instruction::Set(port, value) => {
                    let width = ports
                        .get(port)
                        .unwrap_or_else(|| panic!("No width for port {}", port))
                        .width;
                    let mut bool_values = bitvec_to_vecbool(test_input_to_bitvec(value));
                    bool_values.reverse();
                    bool_values.truncate(width);
                    bool_values.reverse();
                    inputs.create_bus(port, bool_values.len()).unwrap();
                    inputs.insert_option(&Bus::from(port.clone()), bool_values);
                }
                Instruction::Eval => {
                    outputs = simulator.simulate(&inputs)?;
                    print!(".");
                }
                Instruction::Output => {
                    if !(expected[cmp_idx] <= outputs.clone()) {
                        println!("❌ Step: {}", cmp_idx + 1);
                        println!("Expected: {}", expected[cmp_idx]);
                        println!("Actual: {}", outputs);
                        println!();
                        failures += 1;
                    }
                    cmp_idx += 1;
                }
                Instruction::Tick => {
                    outputs = simulator.simulate(&inputs).expect("simulation failure");
                }
                Instruction::Tock => {
                    simulator.tick().expect("Tick failure");
                    outputs = simulator.simulate(&inputs).expect("simulation failure");
                }
            }
        }
    }

    if failures > 0 {
        println!(
            "❌️️️ {} failures, {} successes, {} total. ",
            failures,
            test_script.steps.len() - failures,
            test_script.steps.len()
        );

        return Err(Box::new(N2VError {
            msg: String::from("Test failed."),
            kind: ErrorKind::Other,
        }));
    }

    println!();
    println!("✔️️️    {} tests passed.", test_script.steps.len());
    Ok(())
}

fn read_test(path: &PathBuf) -> Result<String, Box<dyn Error>> {
    Ok(fs::read_to_string(path)?)
}

#[cfg(test)]
mod test {
    use super::*;
    use std::path::Path;

    fn construct_path(path: &PathBuf) -> PathBuf {
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        manifest_dir.join("resources").join("tests").join(path)
    }

    #[test]
    fn test_nand2tetris_solution_not() {
        let path = construct_path(&PathBuf::from("nand2tetris/solutions/Not.tst"));
        assert!(run_test(path.to_str().unwrap()).is_ok());
    }

    #[test]
    fn test_nand2tetris_solution_and() {
        let path = construct_path(&PathBuf::from("nand2tetris/solutions/And.tst"));
        assert!(run_test(path.to_str().unwrap()).is_ok());
    }

    #[test]
    fn test_nand2tetris_solution_or() {
        let path = construct_path(&PathBuf::from("nand2tetris/solutions/Or.tst"));
        assert!(run_test(path.to_str().unwrap()).is_ok());
    }

    #[test]
    fn test_nand2tetris_solution_xor() {
        let path = construct_path(&PathBuf::from("nand2tetris/solutions/Xor.tst"));
        assert!(run_test(path.to_str().unwrap()).is_ok());
    }

    #[test]
    fn test_nand2tetris_solution_mux() {
        let path = construct_path(&PathBuf::from("nand2tetris/solutions/Mux.tst"));
        assert!(run_test(path.to_str().unwrap()).is_ok());
    }

    #[test]
    fn test_nand2tetris_solution_dmux() {
        let path = construct_path(&PathBuf::from("nand2tetris/solutions/DMux.tst"));
        assert!(run_test(path.to_str().unwrap()).is_ok());
    }

    #[test]
    fn test_nand2tetris_solution_not16() {
        let path = construct_path(&PathBuf::from("nand2tetris/solutions/Not16.tst"));
        assert!(run_test(path.to_str().unwrap()).is_ok());
    }

    #[test]
    fn test_nand2tetris_solution_and16() {
        let path = construct_path(&PathBuf::from("nand2tetris/solutions/And16.tst"));
        assert!(run_test(path.to_str().unwrap()).is_ok());
    }

    #[test]
    fn test_nand2tetris_solution_mux16() {
        let path = construct_path(&PathBuf::from("nand2tetris/solutions/Mux16.tst"));
        assert!(run_test(path.to_str().unwrap()).is_ok());
    }

    #[test]
    fn test_nand2tetris_solution_dmux4way() {
        let path = construct_path(&PathBuf::from("nand2tetris/solutions/DMux4Way.tst"));
        assert!(run_test(path.to_str().unwrap()).is_ok());
    }

    #[test]
    fn test_nand2tetris_solution_dmux8way() {
        let path = construct_path(&PathBuf::from("nand2tetris/solutions/DMux4Way.tst"));
        assert!(run_test(path.to_str().unwrap()).is_ok());
    }

    #[test]
    fn test_nand2tetris_solution_mux4way16() {
        let path = construct_path(&PathBuf::from("nand2tetris/solutions/Mux4Way16.tst"));
        assert!(run_test(path.to_str().unwrap()).is_ok());
    }

    #[test]
    fn test_nand2tetris_solution_or8way() {
        let path = construct_path(&PathBuf::from("nand2tetris/solutions/Or8Way.tst"));
        assert!(run_test(path.to_str().unwrap()).is_ok());
    }

    #[test]
    fn test_nand2tetris_solution_halfadder() {
        let path = construct_path(&PathBuf::from("nand2tetris/solutions/HalfAdder.tst"));
        assert!(run_test(path.to_str().unwrap()).is_ok());
    }

    #[test]
    fn test_nand2tetris_solution_fulladder() {
        let path = construct_path(&PathBuf::from("nand2tetris/solutions/HalfAdder.tst"));
        assert!(run_test(path.to_str().unwrap()).is_ok());
    }

    #[test]
    fn test_nand2tetris_solution_alu() {
        let path = construct_path(&PathBuf::from("nand2tetris/solutions/ALU.tst"));
        assert!(run_test(path.to_str().unwrap()).is_ok());
    }

    #[test]
    fn test_nand2tetris_solution_bit() {
        let path = construct_path(&PathBuf::from("nand2tetris/solutions/Bit.tst"));
        assert!(run_test(path.to_str().unwrap()).is_ok());
    }

    #[test]
    fn test_nand2tetris_solution_register() {
        let path = construct_path(&PathBuf::from("nand2tetris/solutions/Register.tst"));
        assert!(run_test(path.to_str().unwrap()).is_ok());
    }

    #[test]
    fn test_nand2tetris_solution_ram8() {
        let path = construct_path(&PathBuf::from("nand2tetris/solutions/RAM8.tst"));
        assert!(run_test(path.to_str().unwrap()).is_ok());
    }

    #[test]
    fn test_nand2tetris_solution_ram512() {
        let path = construct_path(&PathBuf::from("nand2tetris/solutions/RAM512.tst"));
        assert!(run_test(path.to_str().unwrap()).is_ok());
    }

    #[test]
    fn test_nand2tetris_solution_ram4k() {
        let path = construct_path(&PathBuf::from("nand2tetris/solutions/RAM4K.tst"));
        assert!(run_test(path.to_str().unwrap()).is_ok());
    }

    #[test]
    fn test_nand2tetris_solution_ram16k() {
        let path = construct_path(&PathBuf::from("nand2tetris/solutions/RAM16K.tst"));
        assert!(run_test(path.to_str().unwrap()).is_ok());
    }

    #[test]
    fn test_nand2tetris_solution_add16() {
        let path = construct_path(&PathBuf::from("nand2tetris/solutions/Add16.tst"));
        assert!(run_test(path.to_str().unwrap()).is_ok());
    }

    #[test]
    fn test_nand2tetris_solution_inc16() {
        let path = construct_path(&PathBuf::from("nand2tetris/solutions/Inc16.tst"));
        assert!(run_test(path.to_str().unwrap()).is_ok());
    }

    #[test]
    fn test_nand2tetris_solution_pc() {
        let path = construct_path(&PathBuf::from("nand2tetris/solutions/PC.tst"));
        assert!(run_test(path.to_str().unwrap()).is_ok());
    }

    #[test]
    fn test_nand2tetris_solution_cpu() {
        let path = construct_path(&PathBuf::from("nand2tetris/solutions/CPU.tst"));
        assert!(run_test(path.to_str().unwrap()).is_ok());
    }

    #[test]
    fn test_arm_add16() {
        let path = construct_path(&PathBuf::from("arm/Add16.tst"));
        assert!(run_test(path.to_str().unwrap()).is_ok());
    }

    #[test]
    fn test_arm_ops_mux8way3() {
        let path = construct_path(&PathBuf::from("arm/Mux8Way3.tst"));
        assert!(run_test(path.to_str().unwrap()).is_ok());
    }
}
