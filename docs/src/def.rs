use bumpalo::{collections::String as BumpString, Bump};
use roc_ast::{
    ast_error::ASTResult,
    lang::{self, core::def::def_to_def2::def_to_def2},
    mem_pool::pool::Pool,
};
use roc_code_markup::{markup::nodes::def2_to_markup, slow_pool::SlowPool};
use roc_module::symbol::{IdentIds, Interns, ModuleId, ModuleIds};
use roc_region::all::Region;
use roc_types::subs::VarStore;

use crate::html::mark_node_to_html;

// html is written to buf
pub fn defs_to_html<'a>(
    buf: &mut BumpString<'a>,
    defs: Vec<roc_parse::ast::Def<'a>>,
    env_module_id: ModuleId,
    env_module_ids: &'a ModuleIds,
    interns: &Interns,
) {
    let mut env_pool = Pool::with_capacity(1024);
    let env_arena = Bump::new();

    let mut var_store = VarStore::default();
    let dep_idents = IdentIds::exposed_builtins(8);
    let exposed_ident_ids = IdentIds::default();

    let def_arena = Bump::new();

    let mut env = lang::env::Env::new(
        env_module_id,
        &env_arena,
        &mut env_pool,
        &mut var_store,
        dep_idents,
        env_module_ids,
        exposed_ident_ids,
    );

    let mut scope = lang::scope::Scope::new(env.home, env.pool, env.var_store);
    let region = Region::new(0, 0, 0, 0);

    for def in defs.iter() {
        // TODO remove unwrap
        write_def_to_bump_str_html(&def_arena, &mut env, &mut scope, region, def, interns, buf)
            .unwrap();
    }
}

fn write_def_to_bump_str_html<'a, 'b>(
    arena: &'a Bump,
    env: &mut lang::env::Env<'a>,
    scope: &mut lang::scope::Scope,
    region: Region,
    def: &'a roc_parse::ast::Def<'a>,
    interns: &Interns,
    buf: &mut BumpString<'b>,
) -> ASTResult<()> {
    let def2 = def_to_def2(arena, env, scope, def, region);

    let mut def2_pool = Pool::with_capacity(1024);
    let def2_id = def2_pool.add(def2);

    let mut mark_node_pool = SlowPool::default();

    let def2_markup_id = def2_to_markup(
        env,
        def2_pool.get(def2_id),
        def2_id,
        &mut mark_node_pool,
        interns,
    )?;

    let def2_markup_node = mark_node_pool.get(def2_markup_id);

    mark_node_to_html(def2_markup_node, &mark_node_pool, buf);

    Ok(())
}
