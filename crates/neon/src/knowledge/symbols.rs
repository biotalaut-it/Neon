use quote::ToTokens;
use syn::{spanned::Spanned, visit::Visit};

#[derive(Debug, Clone)]
pub(crate) struct SymbolRecord {
    pub name: String,
    pub kind: String,
    pub signature: String,
    pub line: u32,
}
#[derive(Debug, Clone)]
pub(crate) struct DependencyRecord {
    pub to_name: String,
    pub kind: String,
    pub line: u32,
}

pub(crate) fn extract_rust(text: &str) -> (Vec<SymbolRecord>, Vec<DependencyRecord>) {
    let Ok(file) = syn::parse_file(text) else {
        return (Vec::new(), Vec::new());
    };
    let mut visitor = Collector {
        symbols: Vec::new(),
        dependencies: Vec::new(),
    };
    visitor.visit_file(&file);
    (visitor.symbols, visitor.dependencies)
}

struct Collector {
    symbols: Vec<SymbolRecord>,
    dependencies: Vec<DependencyRecord>,
}
impl Collector {
    fn push_symbol(&mut self, name: String, kind: &str, signature: String, line: usize) {
        let signature = signature.chars().take(512).collect::<String>();
        self.symbols.push(SymbolRecord {
            name,
            kind: kind.into(),
            signature,
            line: line.min(u32::MAX as usize) as u32,
        });
    }
}
impl<'ast> Visit<'ast> for Collector {
    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        self.push_symbol(
            item.sig.ident.to_string(),
            "function",
            item.sig.to_token_stream().to_string(),
            item.span().start().line,
        );
        syn::visit::visit_item_fn(self, item);
    }
    fn visit_item_struct(&mut self, item: &'ast syn::ItemStruct) {
        self.push_symbol(
            item.ident.to_string(),
            "struct",
            format!("struct {}", item.ident),
            item.span().start().line,
        );
        syn::visit::visit_item_struct(self, item);
    }
    fn visit_item_enum(&mut self, item: &'ast syn::ItemEnum) {
        self.push_symbol(
            item.ident.to_string(),
            "enum",
            format!("enum {}", item.ident),
            item.span().start().line,
        );
        syn::visit::visit_item_enum(self, item);
    }
    fn visit_item_trait(&mut self, item: &'ast syn::ItemTrait) {
        self.push_symbol(
            item.ident.to_string(),
            "trait",
            format!("trait {}", item.ident),
            item.span().start().line,
        );
        syn::visit::visit_item_trait(self, item);
    }
    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        let name = item.ident.to_string();
        self.push_symbol(
            name.clone(),
            "module",
            format!("mod {name}"),
            item.span().start().line,
        );
        self.dependencies.push(DependencyRecord {
            to_name: name,
            kind: "mod".into(),
            line: item.span().start().line.min(u32::MAX as usize) as u32,
        });
        syn::visit::visit_item_mod(self, item);
    }
    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        self.dependencies.push(DependencyRecord {
            to_name: item.to_token_stream().to_string(),
            kind: "use".into(),
            line: item.span().start().line.min(u32::MAX as usize) as u32,
        });
        syn::visit::visit_item_use(self, item);
    }
}
