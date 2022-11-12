#![allow(clippy::not_unsafe_ptr_arg_deref)]

use std::path::PathBuf;

use glob::glob;
use regex::{escape, Regex};
use swc_core::common::DUMMY_SP;
use swc_core::ecma::ast::{
    BindingIdent, Decl, Expr, Ident, ImportDecl, ImportDefaultSpecifier, ImportSpecifier,
    KeyValueProp, Module, ModuleDecl, ModuleItem, ObjectLit, Pat, Program, Prop, PropName,
    PropOrSpread, Stmt, Str, VarDecl, VarDeclKind, VarDeclarator,
};
use swc_core::ecma::visit::{Fold, FoldWith};
use swc_core::plugin::{
    metadata::TransformPluginMetadataContextKind, plugin_transform,
    proxies::TransformPluginProgramMetadata,
};

pub struct GlobImporter {
    file_name: PathBuf,
    id_counter: usize,
}

struct WildcardImport {
    ident_import: Ident,
    ident_obj: String,
    import_src: String,
}

impl GlobImporter {
    fn is_valid_wildcard_import(&self, decl: &ImportDecl) -> bool {
        decl.src.value.matches('*').count() == 1
    }

    fn expand_wildcard(&mut self, decl: ImportDecl) -> Vec<WildcardImport> {
        let binding = self.file_name.with_file_name(decl.src.value.to_string());
        let pattern = binding.to_str().unwrap();

        let re = Regex::new(&escape(pattern).replace(r"\*", "(.*)")).unwrap();

        glob(pattern)
            .expect("Failed to read glob pattern")
            .filter_map(|e| e.ok())
            .map(|path| {
                let caps = re.captures(path.to_str().unwrap()).unwrap();
                let variable_filename_part = caps.get(1).unwrap().as_str();

                let relative_path = path
                    .strip_prefix(self.file_name.parent().unwrap())
                    .unwrap()
                    .to_str()
                    .unwrap();

                WildcardImport {
                    ident_import: self.next_variable_id(),
                    ident_obj: self.create_valid_property_name(variable_filename_part),
                    import_src: format!("\"./{}\"", relative_path),
                }
            })
            .collect()
    }

    fn create_valid_property_name(&self, ident: &str) -> String {
        let re = Regex::new(r"[^a-zA-Z0-9_]+").unwrap();
        let re2 = Regex::new(r"_+").unwrap();

        re2.replace_all(
            re.replace_all(&ident.replace('-', "_"), "")
                .to_owned()
                .trim_matches('_'),
            "_",
        )
        .into_owned()
    }

    fn split_wildcard_import(&mut self, decl: ImportDecl) -> Vec<ModuleItem> {
        let binding_foo = match decl.specifiers.first() {
            Some(ImportSpecifier::Default(x)) => x.local.sym.to_string(),
            Some(_) => panic!("TODO2"),
            None => panic!("TODO3"),
        };

        let mut results = vec![];
        let expanded = self.expand_wildcard(decl);

        for import in expanded.iter() {
            results.push(ModuleItem::ModuleDecl(ModuleDecl::Import(ImportDecl {
                span: DUMMY_SP,
                specifiers: vec![ImportSpecifier::Default(ImportDefaultSpecifier {
                    span: DUMMY_SP,
                    local: import.ident_import.clone(),
                })],
                src: Box::new(Str {
                    span: DUMMY_SP,
                    raw: Some(import.import_src.clone().into()),
                    value: import.import_src.clone().into(),
                }),
                type_only: false,
                asserts: None,
            })))
        }

        let url_map = ModuleItem::Stmt(Stmt::Decl(Decl::Var(Box::new(VarDecl {
            kind: VarDeclKind::Const,
            declare: false,
            decls: vec![VarDeclarator {
                span: DUMMY_SP,
                definite: false,
                name: Pat::Ident(BindingIdent {
                    id: Ident {
                        span: DUMMY_SP,
                        sym: binding_foo.into(),
                        optional: false,
                    },
                    type_ann: None,
                }),
                init: Some(Box::new(Expr::Object(ObjectLit {
                    span: DUMMY_SP,
                    props: expanded
                        .iter()
                        .map(|i| {
                            PropOrSpread::Prop(Box::new(Prop::KeyValue(KeyValueProp {
                                key: PropName::Ident(Ident {
                                    span: DUMMY_SP,
                                    optional: false,
                                    sym: i.ident_obj.clone().into(),
                                }),
                                value: Box::new(Expr::Ident(i.ident_import.clone())),
                            })))
                        })
                        .collect(),
                }))),
            }],
            span: DUMMY_SP,
        }))));
        results.push(url_map);

        results
    }

    fn next_variable_id(&mut self) -> Ident {
        self.id_counter += 1;
        Ident::new(format!("$_import_{}", self.id_counter).into(), DUMMY_SP)
    }
}

impl Fold for GlobImporter {
    fn fold_module(&mut self, mut module: Module) -> Module {
        let mut new_items: Vec<ModuleItem> = vec![];
        for item in module.body {
            match item {
                ModuleItem::ModuleDecl(ModuleDecl::Import(decl))
                    if self.is_valid_wildcard_import(&decl) =>
                {
                    new_items.extend(self.split_wildcard_import(decl).into_iter());
                }
                _ => {
                    new_items.push(item);
                }
            }
        }
        module.body = new_items;
        module
    }
}

pub fn glob_importer(file_name: PathBuf) -> GlobImporter {
    GlobImporter {
        file_name,
        id_counter: 0,
    }
}

#[plugin_transform]
pub fn process_transform(program: Program, metadata: TransformPluginProgramMetadata) -> Program {
    let file_name = metadata
        .get_context(&TransformPluginMetadataContextKind::Filename)
        .map(PathBuf::from)
        .expect("TODO");

    let mut importer = glob_importer(file_name);
    program.fold_with(&mut importer)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use swc_core::common::{chain, Mark};
    use swc_core::ecma::transforms::base::resolver;
    use swc_core::ecma::transforms::testing::{test, test_fixture};
    use swc_core::testing::fixture;

    use super::glob_importer;

    #[fixture("tests/fixture/**/input.js")]
    fn fixture(input: PathBuf) {
        let output = input.with_file_name("output.js");
        test_fixture(
            Default::default(),
            &|_| {
                chain!(
                    resolver(Mark::new(), Mark::new(), false),
                    glob_importer(input.clone())
                )
            },
            &input,
            &output,
        );
    }
}
