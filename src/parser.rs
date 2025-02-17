use crate::error::{ErrorKind, N2VError};
use crate::expr::*;
use crate::scanner::Token;
use crate::scanner::TokenType;
use crate::Scanner;
use serde::Serialize;
use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;

#[derive(Clone)]
#[allow(clippy::large_enum_variant)]
pub enum Part {
    Component(Component),
    Loop(Loop),
}

/// The Parse Tree for an HDL Chip.
///
#[derive(Clone)]
pub struct ChipHDL {
    pub name: String,
    pub ports: Vec<GenericPort>,
    pub parts: Vec<Part>,
    pub path: Option<PathBuf>,
    pub generic_decls: Vec<Identifier>,
}

impl std::fmt::Display for ChipHDL {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}: PORTS({:?})", self.name, self.ports)
    }
}

impl ChipHDL {
    pub fn get_port(&self, name: &str) -> Result<&GenericPort, Box<dyn Error>> {
        let port_idx = self.ports.iter().position(|x| x.name.value == name);

        match port_idx {
            Some(idx) => Ok(&self.ports[idx]),
            None => Err(Box::new(N2VError {
                msg: format!("Attempt to get non-existent port {}", name),
                kind: ErrorKind::Other,
            })),
        }
    }
}

pub trait HdlProvider {
    fn get_hdl(&self, file_name: &str) -> Result<String, std::io::Error>;
    fn get_path(&self, file_name: &str) -> PathBuf;
}

pub struct FileReader {
    base_path: PathBuf,
}

impl FileReader {
    pub fn new(base_path: &str) -> FileReader {
        if base_path.is_empty() {
            panic!("empty basepath, start file paths in the same directory with ./");
        }
        FileReader {
            base_path: PathBuf::from(base_path),
        }
    }
}

impl HdlProvider for FileReader {
    fn get_hdl(&self, file_name: &str) -> Result<String, std::io::Error> {
        let path = self.base_path.join(file_name);
        let s = fs::read_to_string(&path);
        if let Err(e) = s {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!(
                    "Unable to get HDL for {:?}. {} {:?}",
                    path, e, self.base_path
                ),
            ));
        }
        s
    }

    fn get_path(&self, file_name: &str) -> PathBuf {
        self.base_path.join(file_name)
    }
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq, Hash)]
pub struct Identifier {
    pub value: String,
    pub path: Option<PathBuf>, // Set to None if chip not read from disk, e.g. NAND and DFF.
    pub line: Option<u32>,
}

impl From<Token> for Identifier {
    fn from(t: Token) -> Self {
        if t.token_type != TokenType::Identifier {
            panic!("Attempt to create Identifier from non-identifier token");
        }

        Identifier {
            value: t.lexeme,
            path: Some(t.path),
            line: Some(t.line),
        }
    }
}

impl From<&str> for Identifier {
    fn from(t: &str) -> Self {
        Identifier {
            value: String::from(t),
            path: None,
            line: None,
        }
    }
}

#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PortDirection {
    In,
    Out,
}

#[derive(Serialize, Clone, PartialEq, Eq, Hash, Debug)]
pub struct GenericPort {
    pub name: Identifier,
    pub width: GenericWidth,
    pub direction: PortDirection,
}

#[derive(Clone)]
pub struct Component {
    pub name: Identifier,
    pub mappings: Vec<PortMapping>,
    pub generic_params: Vec<GenericWidth>,
}

#[derive(Clone)]
pub struct Loop {
    pub start: GenericWidth,
    pub end: GenericWidth,
    pub iterator: Identifier,
    pub body: Vec<Component>, // Prevent nested loops.
}

#[derive(Serialize, Clone, PartialEq, Eq, Hash, Debug)]
pub struct BusHDL {
    pub name: String,
    pub start: Option<GenericWidth>,
    pub end: Option<GenericWidth>,
}

//  Not(in=sel, out=notSel); has two wires { name : "sel", port: "in" }, { name : "notSel", port: "out" }
#[derive(Clone)]
pub struct PortMapping {
    pub wire_ident: Identifier,
    pub wire: BusHDL,
    pub port: BusHDL,
}

/// Looks up chip definition for a chip.
/// name is the name of the chip, not including .hdl extension
/// provider is responsible for retrieving the HDL file (provider will have its own base path)
pub fn get_hdl(name: &str, provider: &Rc<dyn HdlProvider>) -> Result<ChipHDL, Box<dyn Error>> {
    if name.to_lowercase() == "nand" {
        // Hard-coded NAND chip
        return Ok(ChipHDL {
            name: String::from("NAND"),
            ports: vec![
                GenericPort {
                    name: Identifier::from("a"),
                    width: GenericWidth::Terminal(Terminal::Num(1)),
                    direction: PortDirection::In,
                },
                GenericPort {
                    name: Identifier::from("b"),
                    width: GenericWidth::Terminal(Terminal::Num(1)),
                    direction: PortDirection::In,
                },
                GenericPort {
                    name: Identifier::from("out"),
                    width: GenericWidth::Terminal(Terminal::Num(1)),
                    direction: PortDirection::Out,
                },
            ],
            parts: Vec::new(),
            path: None,
            generic_decls: Vec::new(),
        });
    } else if name.to_lowercase() == "dff" {
        // Hard-coded NAND chip
        return Ok(ChipHDL {
            name: String::from("DFF"),
            ports: vec![
                GenericPort {
                    name: Identifier::from("in"),
                    width: GenericWidth::Terminal(Terminal::Num(1)),
                    direction: PortDirection::In,
                },
                GenericPort {
                    name: Identifier::from("out"),
                    width: GenericWidth::Terminal(Terminal::Num(1)),
                    direction: PortDirection::Out,
                },
            ],
            parts: Vec::new(),
            path: None,
            generic_decls: Vec::new(),
        });
    }

    let filename = String::from(name) + ".hdl";
    let path = PathBuf::from(filename);

    let contents = provider.get_hdl(path.to_str().unwrap())?;
    let mut scanner = Scanner::new(contents.as_str(), path);
    let mut parser = Parser {
        scanner: &mut scanner,
    };
    parser.parse()
}

pub struct Parser<'a, 'b> {
    pub scanner: &'a mut Scanner<'b>,
}

impl<'a, 'b> Parser<'a, 'b> {
    pub fn parse(&mut self) -> Result<ChipHDL, Box<dyn Error>> {
        self.chip()
    }

    fn consume(&mut self, tt: TokenType) -> Result<Token, Box<dyn Error>> {
        let t = self.scanner.next();
        match &t {
            None => Err(Box::new(N2VError {
                msg: format!("Early end of file, expected {}", tt),
                kind: ErrorKind::ParseError(Token {
                    lexeme: String::from(""),
                    path: self.scanner.path.clone(),
                    line: self.scanner.line,
                    start: self.scanner.col,
                    token_type: TokenType::Eof,
                }),
            })),
            Some(t) => {
                if t.token_type == tt {
                    Ok(t.clone())
                } else {
                    Err(Box::new(N2VError {
                        msg: format!(
                            "I did not expect to see `{}`. I expected to see {}",
                            t.lexeme, tt
                        ),
                        kind: ErrorKind::ParseError(t.clone()),
                    }))
                }
            }
        }
    }

    fn chip(&mut self) -> Result<ChipHDL, Box<dyn Error>> {
        // TODO: Print location information for token.
        self.consume(TokenType::Chip)?;
        let chip_name = self.consume(TokenType::Identifier)?;

        let generics = self.generic_decls()?;

        self.consume(TokenType::LeftCurly)?;

        self.consume(TokenType::In)?;

        let mut ports = self.port_names(PortDirection::In)?;
        self.consume(TokenType::Out)?;

        ports.append(&mut self.port_names(PortDirection::Out)?);

        self.consume(TokenType::Parts)?;
        self.consume(TokenType::Colon)?;

        let parts = self.parts()?;

        // match in ports (can out ports come before in ports?)
        // match out ports
        Ok(ChipHDL {
            name: Identifier::from(chip_name).value,
            ports,
            parts,
            path: Some(self.scanner.path.clone()),
            generic_decls: generics,
        })
    }

    fn generics(&mut self) -> Result<Vec<GenericWidth>, Box<dyn Error>> {
        let mut res: Vec<GenericWidth> = Vec::new();

        if self.scanner.peek().unwrap().token_type != TokenType::LeftAngle {
            return Ok(Vec::new());
        }
        self.consume(TokenType::LeftAngle)?;

        loop {
            let next = self.scanner.next();
            match &next {
                Some(
                    t @ Token {
                        token_type: TokenType::Number,
                        ..
                    },
                ) => {
                    // Convert to number.
                    let val: usize = t.lexeme.parse().unwrap();
                    res.push(GenericWidth::Terminal(Terminal::Num(val)));
                }
                Some(
                    t @ Token {
                        token_type: TokenType::Identifier,
                        ..
                    },
                ) => {
                    res.push(GenericWidth::Terminal(Terminal::Var(Identifier {
                        line: Some(t.line),
                        path: Some(t.path.clone()),
                        value: t.lexeme.clone(),
                    })));
                }
                Some(Token {
                    token_type: TokenType::Comma,
                    ..
                }) => {
                    continue;
                }
                Some(Token {
                    token_type: TokenType::RightAngle,
                    ..
                }) => {
                    return Ok(res);
                }
                Some(t) => {
                    return Err(Box::new(N2VError {
                        msg: format!(
                            "Expected identifier, number, comma, or right angle, found {}",
                            match &next {
                                None => String::from("End of file"),
                                Some(t) => t.token_type.to_string(),
                            }
                        ),
                        kind: ErrorKind::ParseError(t.clone()),
                    }));
                }
                None => {
                    return Err(Box::new(N2VError {
                        msg: String::from(
                            "Unexpected end of file. Expected number, comma, or right angle.",
                        ),
                        kind: ErrorKind::ParseError(Token {
                            lexeme: String::from(""),
                            path: self.scanner.path.clone(),
                            line: self.scanner.line,
                            start: self.scanner.col,
                            token_type: TokenType::Eof,
                        }),
                    }));
                }
            }
        }
    }

    fn generic_decls(&mut self) -> Result<Vec<Identifier>, Box<dyn Error>> {
        let mut res = Vec::new();

        if self.scanner.peek().unwrap().token_type != TokenType::LeftAngle {
            return Ok(Vec::new());
        }
        self.consume(TokenType::LeftAngle)?;

        loop {
            let next = self.scanner.next();
            match &next {
                Some(
                    t @ Token {
                        token_type: TokenType::Identifier,
                        ..
                    },
                ) => {
                    res.push(Identifier {
                        line: Some(t.line),
                        path: Some(t.path.clone()),
                        value: t.lexeme.clone(),
                    });
                }
                Some(Token {
                    token_type: TokenType::Comma,
                    ..
                }) => {
                    continue;
                }
                Some(Token {
                    token_type: TokenType::RightAngle,
                    ..
                }) => {
                    return Ok(res);
                }
                Some(t) => {
                    return Err(Box::new(N2VError {
                        msg: String::from("Expected identifier, comma, or right angle"),
                        kind: ErrorKind::ParseError(t.clone()),
                    }));
                }
                None => {
                    return Err(Box::new(N2VError {
                        msg: String::from(
                            "Unexpected end of file. Expected identifier, comma, or right angle.",
                        ),
                        kind: ErrorKind::ParseError(Token {
                            lexeme: String::from(""),
                            path: self.scanner.path.clone(),
                            line: self.scanner.line,
                            start: self.scanner.col,
                            token_type: TokenType::Eof,
                        }),
                    }));
                }
            }
        }
    }

    fn port_names(&mut self, direction: PortDirection) -> Result<Vec<GenericPort>, Box<dyn Error>> {
        let mut res = Vec::new();

        loop {
            let next = self.scanner.next();
            match &next {
                Some(
                    t @ Token {
                        token_type: TokenType::Identifier,
                        ..
                    },
                ) => {
                    let p = GenericPort {
                        name: Identifier::from(t.clone()),
                        width: self.port_width()?,
                        direction,
                    };
                    res.push(p);
                }
                Some(Token {
                    token_type: TokenType::Comma,
                    ..
                }) => {
                    continue;
                }
                Some(Token {
                    token_type: TokenType::Semicolon,
                    ..
                }) => {
                    return Ok(res);
                }
                Some(t) => {
                    return Err(Box::new(N2VError {
                        msg: String::from("Expected identifier, comma, or semicolon."),
                        kind: ErrorKind::ParseError(t.clone()),
                    }));
                }
                None => {
                    return Err(Box::new(N2VError {
                        msg: String::from(
                            "Unexpected end of file. Expected identifier, comma, or semicolon.",
                        ),
                        kind: ErrorKind::ParseError(Token {
                            lexeme: String::from(""),
                            path: self.scanner.path.clone(),
                            line: self.scanner.line,
                            start: self.scanner.col,
                            token_type: TokenType::Eof,
                        }),
                    }));
                }
            }
        }
    }

    // Parses a list of components (parts). This list may contain for-generate loops.
    fn parts(&mut self) -> Result<Vec<Part>, Box<dyn Error>> {
        let mut parts: Vec<Part> = Vec::new();

        loop {
            let peeked = self.scanner.peek();
            match &peeked {
                Some(Token {
                    token_type: TokenType::Identifier,
                    ..
                }) => {
                    parts.push(Part::Component(self.component()?));
                }
                Some(Token {
                    token_type: TokenType::For,
                    ..
                }) => {
                    parts.push(Part::Loop(self.for_loop()?));
                }
                Some(Token {
                    token_type: TokenType::RightCurly,
                    ..
                }) => {
                    self.scanner.next();
                    break;
                }
                Some(t) => {
                    return Err(Box::new(N2VError {
                        msg: String::from("Expected identifier, FOR, or right curly."),
                        kind: ErrorKind::ParseError(t.clone()),
                    }));
                }
                None => {
                    return Err(Box::new(N2VError {
                        msg: String::from(
                            "Unexpected end of file. Expected identifier, FOR, or right curly.",
                        ),
                        kind: ErrorKind::ParseError(Token {
                            lexeme: String::from(""),
                            path: self.scanner.path.clone(),
                            line: self.scanner.line,
                            start: self.scanner.col,
                            token_type: TokenType::Eof,
                        }),
                    }));
                }
            }
        }

        Ok(parts)
    }

    // Same as parts but does not allow for-generate loops.
    fn components(&mut self) -> Result<Vec<Component>, Box<dyn Error>> {
        let mut parts: Vec<Component> = Vec::new();

        loop {
            let peeked = self.scanner.peek();
            match &peeked {
                Some(Token {
                    token_type: TokenType::Identifier,
                    ..
                }) => {
                    parts.push(self.component()?);
                }
                Some(Token {
                    token_type: TokenType::RightCurly,
                    ..
                }) => {
                    self.scanner.next();
                    break;
                }
                Some(t) => {
                    return Err(Box::new(N2VError {
                        msg: String::from("Expected Identifier or right curly."),
                        kind: ErrorKind::ParseError(t.clone()),
                    }));
                }
                None => {
                    return Err(Box::new(N2VError {
                        msg: String::from(
                            "Unexpected end of file. Expected identifier or right curly.",
                        ),
                        kind: ErrorKind::ParseError(Token {
                            lexeme: String::from(""),
                            path: self.scanner.path.clone(),
                            line: self.scanner.line,
                            start: self.scanner.col,
                            token_type: TokenType::Eof,
                        }),
                    }));
                }
            }
        }

        Ok(parts)
    }

    fn for_loop(&mut self) -> Result<Loop, Box<dyn Error>> {
        self.consume(TokenType::For)?;
        let iterator = Identifier::from(self.consume(TokenType::Identifier)?);
        self.consume(TokenType::In)?;
        let start = self.expr()?;
        self.consume(TokenType::To)?;
        let end = self.expr()?;
        self.consume(TokenType::Generate)?;
        self.consume(TokenType::LeftCurly)?;
        let body = self.components()?;

        Ok(Loop {
            start,
            end,
            iterator,
            body,
        })
    }

    fn expr(&mut self) -> Result<GenericWidth, Box<dyn Error>> {
        let t1 = self.terminal()?;

        let peeked = self.scanner.peek().unwrap();
        if peeked.token_type == TokenType::Plus {
            self.scanner.next();
            let t2 = self.terminal()?;
            Ok(GenericWidth::Expr(
                Op::Add,
                Box::new(GenericWidth::Terminal(t1)),
                Box::new(GenericWidth::Terminal(t2)),
            ))
        } else if peeked.token_type == TokenType::Minus {
            self.scanner.next();
            let t2 = self.terminal()?;
            Ok(GenericWidth::Expr(
                Op::Sub,
                Box::new(GenericWidth::Terminal(t1)),
                Box::new(GenericWidth::Terminal(t2)),
            ))
        } else {
            Ok(GenericWidth::Terminal(t1))
        }
    }

    fn terminal(&mut self) -> Result<Terminal, Box<dyn Error>> {
        let width_token = self.scanner.next().unwrap();
        let width = match width_token.token_type {
            TokenType::Number => Terminal::Num(width_token.lexeme.parse::<usize>().unwrap()),
            TokenType::Identifier => Terminal::Var(Identifier::from(width_token)),
            _ => {
                return Err(Box::new(N2VError {
                    msg: String::from("Expected number or generic var for port width."),
                    kind: ErrorKind::ParseError(width_token),
                }));
            }
        };
        Ok(width)
    }

    fn component(&mut self) -> Result<Component, Box<dyn Error>> {
        Ok(Component {
            name: Identifier::from(self.scanner.next().unwrap()),
            generic_params: self.generics()?,
            mappings: self.port_mappings()?,
        })
    }

    fn port_width(&mut self) -> Result<GenericWidth, Box<dyn Error>> {
        let peeked = self.scanner.peek().unwrap();
        if peeked.token_type != TokenType::LeftBracket {
            return Ok(GenericWidth::Terminal(Terminal::Num(1)));
        }

        self.consume(TokenType::LeftBracket)?;
        let width = self.expr()?;
        self.consume(TokenType::RightBracket)?;

        Ok(width)
    }

    fn bus_idx(&mut self) -> Result<(Option<GenericWidth>, Option<GenericWidth>), Box<dyn Error>> {
        let peeked = self.scanner.peek();

        if let Token {
            token_type: TokenType::LeftBracket,
            ..
        } = peeked.unwrap()
        {
            self.consume(TokenType::LeftBracket)?;
            let start = self.expr()?;

            let end = if let Token {
                token_type: TokenType::Dot,
                ..
            } = self.scanner.peek().unwrap()
            {
                self.consume(TokenType::Dot)?;
                self.consume(TokenType::Dot)?;
                self.expr()?
            } else {
                start.clone()
            };

            self.consume(TokenType::RightBracket)?;
            Ok((Some(start), Some(end)))
        } else {
            Ok((None, None))
        }
    }

    fn port_mappings(&mut self) -> Result<Vec<PortMapping>, Box<dyn Error>> {
        let mut mappings = Vec::new();

        self.consume(TokenType::LeftParen)?;
        loop {
            let next = self.scanner.next();
            match &next {
                Some(
                    t @ Token {
                        token_type: TokenType::Identifier,
                        ..
                    },
                ) => {
                    let (port_start, port_end) = self.bus_idx()?;
                    self.consume(TokenType::Equal)?;
                    let wire = self.consume(TokenType::Identifier)?;
                    let (wire_start, wire_end) = self.bus_idx()?;

                    mappings.push(PortMapping {
                        wire_ident: Identifier::from(t.clone()),
                        wire: BusHDL {
                            name: wire.lexeme,
                            start: wire_start,
                            end: wire_end,
                        },
                        port: BusHDL {
                            name: t.lexeme.clone(),
                            start: port_start,
                            end: port_end,
                        },
                    });

                    let peeked_type = self.scanner.peek().unwrap().token_type;
                    match peeked_type {
                        TokenType::Comma | TokenType::RightParen => {}
                        _ => {
                            let found_t = self.scanner.peek().unwrap();
                            let found = found_t.lexeme.clone();
                            return Err(Box::new(N2VError {
                                msg: format!("Expected comma or right paren, found {}", found),
                                kind: ErrorKind::ParseError(found_t),
                            }));
                        }
                    }
                }
                Some(Token {
                    token_type: TokenType::Comma,
                    ..
                }) => {
                    continue;
                }
                Some(Token {
                    token_type: TokenType::RightParen,
                    ..
                }) => {
                    break;
                }
                Some(t) => {
                    return Err(Box::new(N2VError {
                        msg: String::from("Expected comma, or right paren"),
                        kind: ErrorKind::ParseError(t.clone()),
                    }));
                }
                None => {
                    return Err(Box::new(N2VError {
                        msg: String::from("Unexpected end of file. Expected comma or right paren."),
                        kind: ErrorKind::ParseError(Token {
                            lexeme: String::from(""),
                            path: self.scanner.path.clone(),
                            line: self.scanner.line,
                            start: self.scanner.col,
                            token_type: TokenType::Eof,
                        }),
                    }));
                }
            }
        }

        self.consume(TokenType::Semicolon)?;

        Ok(mappings)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use std::env;
    use std::fs;
    use std::path::Path;

    fn read_hdl(path: &std::path::Path) -> String {
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let test_file = manifest_dir.join("resources").join("tests").join(path);

        fs::read_to_string(test_file).expect("Unable to read test file.")
    }

    #[test]
    fn test_nand2tetris_solution_mux() {
        let path = PathBuf::from("nand2tetris/solutions/Mux.hdl");
        let contents = read_hdl(&path);
        let mut scanner = Scanner::new(contents.as_str(), path);
        let mut parser = Parser {
            scanner: &mut scanner,
        };
        parser.parse().expect("Parse error");
    }

    #[test]
    fn test_nand2tetris_solution_not16() {
        let path = PathBuf::from("nand2tetris/solutions/Not16.hdl");
        let contents = read_hdl(&path);
        let mut scanner = Scanner::new(contents.as_str(), path);
        let mut parser = Parser {
            scanner: &mut scanner,
        };
        parser.parse().expect("Parse error");
    }

    #[test]
    fn test_nand2tetris_solution_and16() {
        let path = PathBuf::from("nand2tetris/solutions/And16.hdl");
        let contents = read_hdl(&path);
        let mut scanner = Scanner::new(contents.as_str(), path);
        let mut parser = Parser {
            scanner: &mut scanner,
        };
        parser.parse().expect("Parse error");
    }

    #[test]
    fn test_nand2tetris_solution_or8way() {
        let path = PathBuf::from("nand2tetris/solutions/Or8Way.hdl");
        let contents = read_hdl(&path);
        let mut scanner = Scanner::new(contents.as_str(), path);
        let mut parser = Parser {
            scanner: &mut scanner,
        };
        parser.parse().expect("Parse error");
    }

    #[test]
    fn test_nand2tetris_solution_not() {
        let path = PathBuf::from("nand2tetris/solutions/Not.hdl");
        let contents = read_hdl(&path);
        let mut scanner = Scanner::new(contents.as_str(), path);
        let mut parser = Parser {
            scanner: &mut scanner,
        };
        parser.parse().expect("Parse error");
    }

    #[test]
    fn test_nand2tetris_solution_alu() {
        let path = PathBuf::from("nand2tetris/solutions/ALU.hdl");
        let contents = read_hdl(&path);
        let mut scanner = Scanner::new(contents.as_str(), path);
        let mut parser = Parser {
            scanner: &mut scanner,
        };
        parser.parse().expect("Parse error");
    }

    #[test]
    fn test_arm_muxgen() {
        let path = PathBuf::from("arm/MuxGen.hdl");
        let contents = read_hdl(&path);
        let mut scanner = Scanner::new(contents.as_str(), path);
        let mut parser = Parser {
            scanner: &mut scanner,
        };
        parser.parse().expect("Parse error");
    }
}
