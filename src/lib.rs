#![allow(
    clippy::collapsible_else_if,
    clippy::collapsible_if,
    clippy::implicit_hasher,
    clippy::match_same_arms,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::module_name_repetitions,
    clippy::must_use_candidate,
    clippy::similar_names,
    clippy::too_many_lines,
    clippy::not_unsafe_ptr_arg_deref
)]

use glob::glob;
use regex::{escape, Regex};
use std::path::PathBuf;
use std::str::FromStr;
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
    cwd: PathBuf,
    file_name: PathBuf,
    id_counter: usize,
}

#[derive(Debug)]
struct WildcardImport {
    ident_import: Ident,
    ident_obj: String,
    import_src: String,
}

impl GlobImporter {
    fn is_valid_wildcard_import(decl: &ImportDecl) -> bool {
        decl.src.value.matches('*').count() == 1
    }

    fn expand_wildcard(&mut self, decl: &ImportDecl) -> Vec<WildcardImport> {
        let pattern = {
            self.cwd
                .join(self.file_name.clone())
                .with_file_name(decl.src.value.to_string())
        };

        let re = Regex::new(&escape(&decl.src.value).replace(r"\*", "(.*)")).unwrap();
        glob(pattern.to_str().unwrap())
            .expect("Failed to read glob pattern")
            .map(|result| match result {
                Ok(path) => {
                    let caps = re.captures(path.to_str().unwrap()).unwrap();
                    let variable_filename_part = caps.get(1).unwrap().as_str();

                    let xxx = self.cwd.join(self.file_name.parent().unwrap());
                    let relative_path = path.strip_prefix(&xxx).unwrap().to_str().unwrap();

                    WildcardImport {
                        ident_import: self.next_variable_id(),
                        ident_obj: Self::create_valid_property_name(variable_filename_part),
                        import_src: if relative_path.starts_with('.') {
                            relative_path.to_string()
                        } else {
                            format!("./{relative_path}")
                        },
                    }
                }
                Err(e) => panic!("{e:?}"),
            })
            .collect()
    }

    fn create_valid_property_name(ident: &str) -> String {
        let re = regex::Regex::new(r"[^a-zA-Z0-9_]+").unwrap();
        let re2 = regex::Regex::new(r"_+").unwrap();

        re2.replace_all(
            re.replace_all(&ident.replace('-', "_"), "")
                .trim_matches('_'),
            "_",
        )
        .into_owned()
    }

    fn split_wildcard_import(&mut self, decl: &ImportDecl) -> Vec<ModuleItem> {
        let ident = match decl.specifiers.first() {
            Some(ImportSpecifier::Default(x)) => x.local.clone(),
            Some(_) => panic!("TODO2"),
            None => panic!("TODO3"),
        };
        let mut results = vec![];
        let expanded = self.expand_wildcard(decl);

        for import in &expanded {
            results.push(ModuleItem::ModuleDecl(ModuleDecl::Import(ImportDecl {
                span: DUMMY_SP,
                specifiers: vec![ImportSpecifier::Default(ImportDefaultSpecifier {
                    span: DUMMY_SP,
                    local: import.ident_import.clone(),
                })],
                src: Box::new(Str {
                    span: DUMMY_SP,
                    raw: None,
                    value: import.import_src.clone().into(),
                }),
                type_only: false,
                asserts: None,
            })));
        }

        let url_map = ModuleItem::Stmt(Stmt::Decl(Decl::Var(Box::new(VarDecl {
            kind: VarDeclKind::Const,
            declare: false,
            decls: vec![VarDeclarator {
                span: DUMMY_SP,
                definite: false,
                name: Pat::Ident(BindingIdent {
                    id: Ident {
                        span: ident.span,
                        sym: ident.sym,
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
        module.body = module
            .body
            .iter()
            .flat_map(|item| match item {
                ModuleItem::ModuleDecl(ModuleDecl::Import(decl))
                    if Self::is_valid_wildcard_import(decl) =>
                {
                    self.split_wildcard_import(decl)
                }
                _ => vec![item.clone()],
            })
            .collect();

        module
    }
}

pub fn glob_importer(cwd: PathBuf, file_name: PathBuf) -> GlobImporter {
    GlobImporter {
        cwd,
        file_name,
        id_counter: 0,
    }
}

#[plugin_transform]
pub fn process_transform(program: Program, metadata: TransformPluginProgramMetadata) -> Program {
    let file_name = metadata
        .get_context(&TransformPluginMetadataContextKind::Filename)
        .map(PathBuf::from)
        .expect("Plugin requires filename metadata.");

    // swc mounts the current working directory under the /cwd path
    let cwd = PathBuf::from_str("/cwd").unwrap();

    let mut importer = glob_importer(cwd, file_name);
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
        let cwd = input.parent().unwrap().to_path_buf();
        test_fixture(
            Default::default(),
            &|_| {
                chain!(
                    resolver(Mark::new(), Mark::new(), false),
                    glob_importer(cwd.clone(), input.clone())
                )
            },
            &input,
            &output,
        );
    }
}
