use crate::ir::{
    BaseType, Diagnostic, Key, SCHEMA_VERSION, Schema, Type, is_public_prefixed, is_valid_key_name,
};

/// Parse a `.env.schema`. Every problem in the file is reported at once.
pub fn parse(input: &str) -> Result<Schema, Vec<Diagnostic>> {
    let mut p = Parser {
        schema: Schema::default(),
        diags: Vec::new(),
    };
    p.run(input);
    if p.diags.is_empty() {
        Ok(p.schema)
    } else {
        Err(p.diags)
    }
}

struct Parser {
    schema: Schema,
    diags: Vec<Diagnostic>,
}

#[derive(Debug)]
struct Decorator {
    name: String,
    value: Option<String>,
    line: u32,
    column: u32,
}

#[derive(Debug, Default)]
struct Block {
    description: Vec<String>,
    decorators: Vec<Decorator>,
}

impl Block {
    fn is_empty(&self) -> bool {
        self.description.is_empty() && self.decorators.is_empty()
    }

    fn has_header_decorator(&self) -> bool {
        self.decorators.iter().any(|d| {
            matches!(
                d.name.as_str(),
                "penv" | "schema" | "defaultSensitive" | "defaultRequired"
            )
        })
    }
}

impl Parser {
    fn error(&mut self, line: u32, column: u32, code: &str, message: impl Into<String>) {
        self.diags
            .push(Diagnostic::new(line, column, code, message));
    }

    fn run(&mut self, input: &str) {
        let body = input.strip_prefix('\u{feff}').unwrap_or(input);
        let mut block = Block::default();
        let mut first_block_done = false;

        for (idx, raw) in body.split('\n').enumerate() {
            let line_no = idx as u32 + 1;
            let line = raw.strip_suffix('\r').unwrap_or(raw);
            let trimmed = line.trim();

            if trimmed.is_empty() {
                if !block.is_empty() {
                    if !first_block_done {
                        self.take_header(&block);
                    }
                    first_block_done = true;
                }
                block = Block::default();
                continue;
            }

            if let Some(rest) = trimmed.strip_prefix('#') {
                let hash_col = (line.len() - line.trim_start().len()) as u32 + 1;
                self.read_comment(rest, line_no, hash_col + 1, &mut block);
                continue;
            }

            if !first_block_done && block.has_header_decorator() {
                self.take_header(&block);
                block = Block::default();
            }
            self.read_key(line, line_no, &block);
            block = Block::default();
            first_block_done = true;
        }

        if !block.is_empty() && !first_block_done {
            self.take_header(&block);
        }
    }

    fn read_comment(&mut self, rest: &str, line_no: u32, base_col: u32, block: &mut Block) {
        let lead = rest.len() - rest.trim_start().len();
        let body = rest.trim();
        if !body.starts_with('@') {
            if !body.is_empty() {
                block.description.push(body.to_string());
            }
            return;
        }
        self.read_decorators(body, line_no, base_col + lead as u32, block);
    }

    fn read_decorators(&mut self, body: &str, line_no: u32, base_col: u32, block: &mut Block) {
        let chars: Vec<char> = body.chars().collect();
        let mut i = 0usize;
        while i < chars.len() {
            if chars[i].is_whitespace() {
                i += 1;
                continue;
            }
            let column = base_col + i as u32;
            if chars[i] != '@' {
                self.error(
                    line_no,
                    column,
                    "invalid_decorator",
                    format!("line {line_no}: expected a decorator starting with @"),
                );
                return;
            }
            i += 1;
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let name: String = chars[start..i].iter().collect();
            if name.is_empty() {
                self.error(
                    line_no,
                    column,
                    "invalid_decorator",
                    format!("line {line_no}: @ with no decorator name"),
                );
                return;
            }
            let mut value = None;
            if i < chars.len() && chars[i] == '=' {
                i += 1;
                match self.read_value(&chars, &mut i, line_no, base_col) {
                    Some(v) => value = Some(v),
                    None => return,
                }
            }
            block.decorators.push(Decorator {
                name,
                value,
                line: line_no,
                column,
            });
        }
    }

    fn read_value(
        &mut self,
        chars: &[char],
        i: &mut usize,
        line_no: u32,
        base_col: u32,
    ) -> Option<String> {
        if *i >= chars.len() {
            return Some(String::new());
        }
        let quote = chars[*i];
        if quote == '"' || quote == '\'' {
            let open = *i;
            *i += 1;
            let mut out = String::new();
            while *i < chars.len() {
                let c = chars[*i];
                if c == '\\' && quote == '"' && *i + 1 < chars.len() {
                    out.push(chars[*i + 1]);
                    *i += 2;
                    continue;
                }
                if c == quote {
                    *i += 1;
                    return Some(out);
                }
                out.push(c);
                *i += 1;
            }
            self.error(
                line_no,
                base_col + open as u32,
                "unterminated_quote",
                format!("line {line_no}: a quoted decorator value is never closed"),
            );
            return None;
        }
        let start = *i;
        let mut depth = 0usize;
        let mut quoted: Option<char> = None;
        while *i < chars.len() {
            let c = chars[*i];
            match quoted {
                Some(q) if c == q => quoted = None,
                Some(_) => {}
                None if c == '"' || c == '\'' => quoted = Some(c),
                None if c == '(' => depth += 1,
                None if c == ')' => depth = depth.saturating_sub(1),
                None if depth == 0 && c.is_whitespace() => break,
                None => {}
            }
            *i += 1;
        }
        Some(chars[start..*i].iter().collect())
    }

    fn take_header(&mut self, block: &Block) {
        for d in &block.decorators {
            match d.name.as_str() {
                "penv" => match d.value.as_deref().and_then(split_project) {
                    Some((org, project)) => {
                        self.schema.org = Some(org);
                        self.schema.project = Some(project);
                    }
                    None => self.error(
                        d.line,
                        d.column,
                        "invalid_header",
                        format!("line {}: @penv takes org/project", d.line),
                    ),
                },
                "schema" => match d.value.as_deref().and_then(|v| v.parse::<u32>().ok()) {
                    Some(n) if n > 0 => self.schema.schema_version = n,
                    _ => self.error(
                        d.line,
                        d.column,
                        "invalid_header",
                        format!("line {}: @schema takes a version number, such as 1", d.line),
                    ),
                },
                "defaultSensitive" => {
                    if let Some(v) = self.flag(d) {
                        self.schema.default_sensitive = v;
                    }
                }
                "defaultRequired" => {
                    if let Some(v) = self.flag(d) {
                        self.schema.default_required = v;
                    }
                }
                other => self.error(
                    d.line,
                    d.column,
                    "unknown_decorator",
                    format!("line {}: @{other} is not a header decorator", d.line),
                ),
            }
        }
        if self.schema.schema_version > SCHEMA_VERSION {
            let d = block
                .decorators
                .iter()
                .find(|d| d.name == "schema")
                .map(|d| (d.line, d.column))
                .unwrap_or((1, 1));
            self.error(
                d.0,
                d.1,
                "schema_too_new",
                format!(
                    "line {}: this build understands @schema={SCHEMA_VERSION}. Run penv upgrade.",
                    d.0
                ),
            );
        }
    }

    /// A boolean decorator: bare means true, otherwise `true` or `false`.
    fn flag(&mut self, d: &Decorator) -> Option<bool> {
        match d.value.as_deref() {
            None | Some("true") => Some(true),
            Some("false") => Some(false),
            Some(_) => {
                self.error(
                    d.line,
                    d.column,
                    "invalid_decorator_value",
                    format!("line {}: @{} takes true or false", d.line, d.name),
                );
                None
            }
        }
    }

    fn require_value(&mut self, d: &Decorator) -> Option<String> {
        match d.value.as_deref() {
            Some(v) if !v.is_empty() => Some(v.to_string()),
            _ => {
                self.error(
                    d.line,
                    d.column,
                    "missing_decorator_value",
                    format!("line {}: @{} needs a value", d.line, d.name),
                );
                None
            }
        }
    }

    fn read_key(&mut self, line: &str, line_no: u32, block: &Block) {
        let Some(eq) = line.find('=') else {
            self.error(
                line_no,
                1,
                "invalid_line",
                format!("line {line_no}: expected KEY=value or a # comment"),
            );
            return;
        };
        let name = line[..eq].trim().to_string();
        if !is_valid_key_name(&name) {
            self.error(
                line_no,
                1,
                "invalid_key_name",
                format!("line {line_no}: {name:?} is not a usable key name"),
            );
            return;
        }
        if self.schema.get(&name).is_some() {
            self.error(
                line_no,
                1,
                "duplicate_key",
                format!("line {line_no}: {name} is declared twice; keep one block"),
            );
            return;
        }

        let raw_default = unquote(line[eq + 1..].trim());
        let default = if raw_default.is_empty() {
            None
        } else {
            Some(raw_default)
        };

        let mut key = Key {
            name,
            default,
            ..Key::default()
        };
        let mut type_seen = false;

        for d in &block.decorators {
            match d.name.as_str() {
                "type" => {
                    if let Some(v) = self.require_value(d) {
                        type_seen = true;
                        key.ty = self.read_type(&v, d);
                    }
                }
                "required" | "optional" => {
                    if d.value.is_some() {
                        self.error(
                            d.line,
                            d.column,
                            "invalid_decorator_value",
                            format!(
                                "line {}: @{} takes no value; write @required or @optional",
                                d.line, d.name
                            ),
                        );
                    } else {
                        key.required_decorator = Some(d.name == "required");
                    }
                }
                "sensitive" => key.sensitive_decorator = self.flag(d),
                "example" => key.example = self.require_value(d),
                "docs" => key.docs = self.require_value(d),
                "since" => key.since = self.require_value(d),
                "deprecated" => key.deprecated = Some(d.value.clone().unwrap_or_default()),
                "rotate" => {
                    if let Some(v) = self.require_value(d) {
                        if is_duration(&v) {
                            key.rotate = Some(v);
                        } else {
                            self.error(
                                d.line,
                                d.column,
                                "invalid_decorator_value",
                                format!("line {}: @rotate takes a duration such as 90d", d.line),
                            );
                        }
                    }
                }
                "dynamic" | "static" => {
                    if d.value.is_some() {
                        self.error(
                            d.line,
                            d.column,
                            "invalid_decorator_value",
                            format!(
                                "line {}: @{} takes no value; write @dynamic or @static",
                                d.line, d.name
                            ),
                        );
                    } else {
                        key.dynamic = Some(d.name == "dynamic");
                    }
                }
                "dynamicFrom" => key.dynamic_from = self.require_value(d),
                other => self.error(
                    d.line,
                    d.column,
                    "unknown_decorator",
                    format!("line {}: @{other} is not a decorator penv knows", d.line),
                ),
            }
        }

        if !type_seen {
            key.ty = Type::new(BaseType::String);
        }
        if !block.description.is_empty() {
            key.description = Some(block.description.join(" "));
        }

        let prefixed = is_public_prefixed(&key.name);
        key.sensitive = match key.sensitive_decorator {
            Some(true) => {
                if prefixed {
                    self.error(
                        line_no,
                        1,
                        "sensitive_public_key",
                        format!(
                            "line {line_no}: {} carries a bundler prefix, so its value ships to the browser. Drop @sensitive or rename the key.",
                            key.name
                        ),
                    );
                }
                true
            }
            Some(false) => false,
            None => !prefixed && self.schema.default_sensitive,
        };
        key.required = match key.required_decorator {
            Some(v) => v,
            None => key.default.is_none() && self.schema.default_required,
        };

        self.schema.keys.push(key);
    }

    fn read_type(&mut self, raw: &str, d: &Decorator) -> Type {
        let (name, args) = match raw.find('(') {
            Some(open) => {
                if !raw.ends_with(')') {
                    self.error(
                        d.line,
                        d.column,
                        "invalid_type",
                        format!("line {}: @type is missing its closing bracket", d.line),
                    );
                    return Type::new(BaseType::String);
                }
                (&raw[..open], Some(&raw[open + 1..raw.len() - 1]))
            }
            None => (raw, None),
        };

        let Some(base) = BaseType::from_name(name) else {
            self.error(
                d.line,
                d.column,
                "unknown_type",
                format!(
                    "line {}: {name} is not a penv type. Use string, number, boolean, url, email, port or enum. A whole number is number(isInt=true).",
                    d.line
                ),
            );
            return Type::new(BaseType::String);
        };

        let mut ty = Type::new(base);
        let Some(args) = args else { return ty };
        for arg in split_args(args) {
            let arg = arg.trim();
            if arg.is_empty() {
                continue;
            }
            match arg.split_once('=') {
                Some((k, v)) => {
                    let k = k.trim();
                    let v = unquote(v.trim());
                    if !base.constraints().contains(&k) {
                        self.error(
                            d.line,
                            d.column,
                            "unknown_constraint",
                            format!("line {}: {} takes no {k} constraint", d.line, base.as_str()),
                        );
                        continue;
                    }
                    if ty.constraint(k).is_some() {
                        self.error(
                            d.line,
                            d.column,
                            "duplicate_constraint",
                            format!("line {}: {k} is given twice", d.line),
                        );
                        continue;
                    }
                    ty.constraints.push((k.to_string(), v));
                }
                None => {
                    if base != BaseType::Enum {
                        self.error(
                            d.line,
                            d.column,
                            "invalid_type",
                            format!(
                                "line {}: {} takes name=value constraints, not bare arguments",
                                d.line,
                                base.as_str()
                            ),
                        );
                        continue;
                    }
                    ty.members.push(unquote(arg));
                }
            }
        }
        if base == BaseType::Enum && ty.members.is_empty() {
            self.error(
                d.line,
                d.column,
                "invalid_type",
                format!("line {}: enum needs at least one member", d.line),
            );
        }
        ty.constraints.sort_by(|a, b| a.0.cmp(&b.0));
        ty
    }
}

fn split_project(v: &str) -> Option<(String, String)> {
    let (org, project) = v.split_once('/')?;
    if org.is_empty() || project.is_empty() || project.contains('/') {
        return None;
    }
    Some((org.to_string(), project.to_string()))
}

/// Split on commas that are not inside quotes.
fn split_args(args: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    for c in args.chars() {
        match quote {
            Some(q) => {
                current.push(c);
                if c == q {
                    quote = None;
                }
            }
            None if c == '"' || c == '\'' => {
                quote = Some(c);
                current.push(c);
            }
            None if c == ',' => out.push(std::mem::take(&mut current)),
            None => current.push(c),
        }
    }
    out.push(current);
    out
}

fn unquote(v: &str) -> String {
    let bytes = v.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if (first == b'"' || first == b'\'') && first == last {
            let inner = &v[1..v.len() - 1];
            return if first == b'"' {
                inner.replace("\\\"", "\"").replace("\\\\", "\\")
            } else {
                inner.to_string()
            };
        }
    }
    v.to_string()
}

fn is_duration(v: &str) -> bool {
    let Some(unit) = v.chars().last() else {
        return false;
    };
    if !matches!(unit, 's' | 'm' | 'h' | 'd' | 'w') {
        return false;
    }
    let digits = &v[..v.len() - 1];
    !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit())
}
