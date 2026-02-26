use syn::visit::Visit;
use syn::{
    Arm, Attribute, BinOp, ExprBinary, ExprForLoop, ExprIf, ExprLoop, ExprTry, ExprWhile, File,
    ImplItem, Item, TraitItem,
};

#[derive(Debug, Clone)]
pub struct FunctionInfo {
    pub name: String,
    pub start_line: usize,
    pub end_line: usize,
    pub complexity: u32,
}

/// Extract all functions from Rust source code with their cyclomatic complexity.
pub fn extract_functions(source: &str) -> Vec<FunctionInfo> {
    let syntax: File = syn::parse_file(source).expect("failed to parse Rust source");
    let mut extractor = FunctionExtractor {
        functions: Vec::new(),
        impl_name: None,
    };
    extractor.visit_file(&syntax);
    extractor.functions
}

fn has_test_attr(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|a| a.path().is_ident("test"))
}

fn has_cfg_test_attr(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|a| {
        if !a.path().is_ident("cfg") {
            return false;
        }
        let mut found = false;
        let _ = a.parse_nested_meta(|meta| {
            if meta.path.is_ident("test") {
                found = true;
            }
            Ok(())
        });
        found
    })
}

struct FunctionExtractor {
    functions: Vec<FunctionInfo>,
    impl_name: Option<String>,
}

impl<'ast> Visit<'ast> for FunctionExtractor {
    fn visit_item(&mut self, node: &'ast Item) {
        // Skip #[cfg(test)] modules entirely
        if let Item::Mod(m) = node {
            if has_cfg_test_attr(&m.attrs) {
                return;
            }
        }
        syn::visit::visit_item(self, node);
    }

    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        if !has_test_attr(&node.attrs) {
            let name = node.sig.ident.to_string();
            let start = node.sig.ident.span().start().line;
            let end = span_end_line(&node.block);
            let complexity = compute_complexity(&node.block);
            self.functions.push(FunctionInfo {
                name,
                start_line: start,
                end_line: end,
                complexity,
            });
        }
        // Visit statements to find nested fn items (they're extracted separately)
        for stmt in &node.block.stmts {
            if let syn::Stmt::Item(item) = stmt {
                self.visit_item(item);
            }
        }
    }

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        let name = type_name(&node.self_ty);
        let prev = self.impl_name.take();
        self.impl_name = Some(name);
        for item in &node.items {
            self.visit_impl_item(item);
        }
        self.impl_name = prev;
    }

    fn visit_impl_item(&mut self, node: &'ast ImplItem) {
        if let ImplItem::Fn(method) = node {
            if !has_test_attr(&method.attrs) {
                let base = method.sig.ident.to_string();
                let name = if let Some(ref impl_name) = self.impl_name {
                    format!("{impl_name}::{base}")
                } else {
                    base
                };
                let start = method.sig.ident.span().start().line;
                let end = span_end_line(&method.block);
                let complexity = compute_complexity(&method.block);
                self.functions.push(FunctionInfo {
                    name,
                    start_line: start,
                    end_line: end,
                    complexity,
                });
            }
        }
    }

    fn visit_item_trait(&mut self, node: &'ast syn::ItemTrait) {
        for item in &node.items {
            if let TraitItem::Fn(method) = item {
                if let Some(ref block) = method.default {
                    if !has_test_attr(&method.attrs) {
                        let name = method.sig.ident.to_string();
                        let start = method.sig.ident.span().start().line;
                        let end = span_end_line(block);
                        let complexity = compute_complexity(block);
                        self.functions.push(FunctionInfo {
                            name,
                            start_line: start,
                            end_line: end,
                            complexity,
                        });
                    }
                }
            }
        }
    }
}

fn type_name(ty: &syn::Type) -> String {
    match ty {
        syn::Type::Path(tp) => tp
            .path
            .segments
            .iter()
            .map(|s| s.ident.to_string())
            .collect::<Vec<_>>()
            .join("::"),
        _ => "<impl>".to_string(),
    }
}

fn span_end_line(block: &syn::Block) -> usize {
    block.brace_token.span.close().end().line
}

fn compute_complexity(block: &syn::Block) -> u32 {
    let mut visitor = ComplexityVisitor { complexity: 1 };
    visitor.visit_block(block);
    visitor.complexity
}

struct ComplexityVisitor {
    complexity: u32,
}

impl<'ast> Visit<'ast> for ComplexityVisitor {
    fn visit_expr_if(&mut self, node: &'ast ExprIf) {
        self.complexity += 1;
        self.visit_expr(&node.cond);
        self.visit_block(&node.then_branch);
        if let Some((_, ref else_branch)) = node.else_branch {
            self.visit_expr(else_branch);
        }
    }

    fn visit_expr_while(&mut self, node: &'ast ExprWhile) {
        self.complexity += 1;
        syn::visit::visit_expr_while(self, node);
    }

    fn visit_expr_for_loop(&mut self, node: &'ast ExprForLoop) {
        self.complexity += 1;
        syn::visit::visit_expr_for_loop(self, node);
    }

    fn visit_expr_loop(&mut self, node: &'ast ExprLoop) {
        self.complexity += 1;
        syn::visit::visit_expr_loop(self, node);
    }

    fn visit_arm(&mut self, node: &'ast Arm) {
        self.complexity += 1;
        syn::visit::visit_arm(self, node);
    }

    fn visit_expr_binary(&mut self, node: &'ast ExprBinary) {
        match node.op {
            BinOp::And(_) | BinOp::Or(_) => {
                self.complexity += 1;
            }
            _ => {}
        }
        syn::visit::visit_expr_binary(self, node);
    }

    fn visit_expr_try(&mut self, node: &'ast ExprTry) {
        self.complexity += 1;
        syn::visit::visit_expr_try(self, node);
    }

    // Don't recurse into nested fn items — they have their own complexity
    fn visit_item_fn(&mut self, _node: &'ast syn::ItemFn) {}

    // But DO recurse into closures — they contribute to parent's complexity
    fn visit_expr_closure(&mut self, node: &'ast syn::ExprClosure) {
        syn::visit::visit_expr_closure(self, node);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cc(source: &str) -> u32 {
        let fns = extract_functions(source);
        assert_eq!(fns.len(), 1, "expected exactly 1 function, got: {fns:?}");
        fns[0].complexity
    }

    #[test]
    fn empty_function() {
        assert_eq!(cc("fn foo() {}"), 1);
    }

    #[test]
    fn no_branches() {
        assert_eq!(cc("fn foo(x: i32) -> i32 { x + 1 }"), 1);
    }

    #[test]
    fn single_if() {
        assert_eq!(cc("fn foo(x: bool) -> i32 { if x { 1 } else { 0 } }"), 2);
    }

    #[test]
    fn if_let() {
        assert_eq!(
            cc("fn foo(x: Option<i32>) -> i32 { if let Some(v) = x { v } else { 0 } }"),
            2
        );
    }

    #[test]
    fn while_loop() {
        assert_eq!(
            cc("fn foo() { let mut i = 0; while i < 10 { i += 1; } }"),
            2
        );
    }

    #[test]
    fn while_let() {
        assert_eq!(
            cc("fn foo(mut v: Vec<i32>) { while let Some(_) = v.pop() {} }"),
            2
        );
    }

    #[test]
    fn for_loop() {
        assert_eq!(cc("fn foo() { for _i in 0..10 {} }"), 2);
    }

    #[test]
    fn loop_expr() {
        assert_eq!(cc("fn foo() { loop { break; } }"), 2);
    }

    #[test]
    fn match_arms() {
        assert_eq!(
            cc("fn foo(x: i32) -> &'static str { match x { 0 => \"zero\", _ => \"other\" } }"),
            3
        );
    }

    #[test]
    fn match_three_arms() {
        assert_eq!(
            cc("fn foo(x: i32) -> i32 { match x { 1 => 10, 2 => 20, _ => 0 } }"),
            4
        );
    }

    #[test]
    fn logical_and() {
        assert_eq!(cc("fn foo(a: bool, b: bool) -> bool { a && b }"), 2);
    }

    #[test]
    fn logical_or() {
        assert_eq!(cc("fn foo(a: bool, b: bool) -> bool { a || b }"), 2);
    }

    #[test]
    fn try_operator() {
        assert_eq!(
            cc("fn foo() -> Result<i32, ()> { let x = Err(())?; Ok(x) }"),
            2
        );
    }

    #[test]
    fn combined_decision_points() {
        // if (+1), if (+1), && (+1) = base 1 + 3 = 4
        let src = r#"
fn foo(x: bool, y: bool) -> i32 {
    if x {
        if x && y {
            1
        } else {
            2
        }
    } else {
        0
    }
}
"#;
        assert_eq!(cc(src), 4);
    }

    #[test]
    fn closure_contributes_to_parent() {
        let src = r#"
fn foo(items: Vec<i32>) -> Vec<i32> {
    items.into_iter().filter(|x| if *x > 0 { true } else { false }).collect()
}
"#;
        assert_eq!(cc(src), 2);
    }

    #[test]
    fn nested_fn_extracted_separately() {
        let src = r#"
fn outer() {
    fn inner(x: bool) -> i32 {
        if x { 1 } else { 0 }
    }
}
"#;
        let fns = extract_functions(src);
        assert_eq!(fns.len(), 2);
        let outer = fns.iter().find(|f| f.name == "outer").unwrap();
        let inner = fns.iter().find(|f| f.name == "inner").unwrap();
        assert_eq!(outer.complexity, 1);
        assert_eq!(inner.complexity, 2);
    }

    #[test]
    fn impl_methods() {
        let src = r#"
struct Foo;
impl Foo {
    fn bar(&self) -> i32 { 42 }
    fn baz(&self, x: bool) -> i32 { if x { 1 } else { 0 } }
}
"#;
        let fns = extract_functions(src);
        assert_eq!(fns.len(), 2);
        let bar = fns.iter().find(|f| f.name == "Foo::bar").unwrap();
        let baz = fns.iter().find(|f| f.name == "Foo::baz").unwrap();
        assert_eq!(bar.complexity, 1);
        assert_eq!(baz.complexity, 2);
    }

    #[test]
    fn skips_test_functions() {
        let src = r#"
fn real_fn() -> i32 { 42 }

#[test]
fn test_fn() {
    assert_eq!(real_fn(), 42);
}
"#;
        let fns = extract_functions(src);
        assert_eq!(fns.len(), 1);
        assert_eq!(fns[0].name, "real_fn");
    }

    #[test]
    fn skips_cfg_test_modules() {
        let src = r#"
fn real_fn() -> i32 { 42 }

#[cfg(test)]
mod tests {
    fn helper() -> i32 { 1 }

    #[test]
    fn test_fn() {
        assert_eq!(super::real_fn(), 42);
    }
}
"#;
        let fns = extract_functions(src);
        assert_eq!(fns.len(), 1);
        assert_eq!(fns[0].name, "real_fn");
    }

    #[test]
    fn trait_default_methods() {
        let src = r#"
trait MyTrait {
    fn required(&self) -> i32;
    fn default_method(&self) -> i32 {
        if true { 1 } else { 0 }
    }
}
"#;
        let fns = extract_functions(src);
        assert_eq!(fns.len(), 1);
        assert_eq!(fns[0].name, "default_method");
        assert_eq!(fns[0].complexity, 2);
    }

    #[test]
    fn function_line_numbers() {
        let src = r#"
fn first() -> i32 {
    42
}

fn second(x: bool) -> i32 {
    if x { 1 } else { 0 }
}
"#;
        let fns = extract_functions(src);
        assert_eq!(fns.len(), 2);
        assert_eq!(fns[0].name, "first");
        assert_eq!(fns[0].start_line, 2);
        assert_eq!(fns[0].end_line, 4);
        assert_eq!(fns[1].name, "second");
        assert_eq!(fns[1].start_line, 6);
        assert_eq!(fns[1].end_line, 8);
    }
}
