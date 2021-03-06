// Copyright 2012-2014 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

// A pass that annotates for each loops and functions with the free
// variables that they contain.

#![allow(non_camel_case_types)]

use middle::def;
use middle::resolve;
use middle::ty;
use util::nodemap::{DefIdSet, NodeMap, NodeSet};

use syntax::codemap::Span;
use syntax::{ast};
use syntax::visit;
use syntax::visit::Visitor;

#[deriving(Show)]
pub enum CaptureMode {
    /// Copy/move the value from this llvm ValueRef into the environment.
    CaptureByValue,

    /// Access by reference (used for stack closures).
    CaptureByRef
}

// A vector of defs representing the free variables referred to in a function.
// (The def_upvar will already have been stripped).
#[deriving(Encodable, Decodable)]
pub struct freevar_entry {
    pub def: def::Def, //< The variable being accessed free.
    pub span: Span     //< First span where it is accessed (there can be multiple)
}

pub type freevar_map = NodeMap<Vec<freevar_entry>>;

pub type UnboxedClosureList = DefIdSet;

struct CollectFreevarsVisitor<'a> {
    seen: NodeSet,
    refs: Vec<freevar_entry>,
    def_map: &'a resolve::DefMap,
}

impl<'a> Visitor<int> for CollectFreevarsVisitor<'a> {
    fn visit_item(&mut self, _: &ast::Item, _: int) {
        // ignore_item
    }

    fn visit_expr(&mut self, expr: &ast::Expr, depth: int) {
        match expr.node {
            ast::ExprFnBlock(..) | ast::ExprProc(..) |
            ast::ExprUnboxedFn(..) => {
                visit::walk_expr(self, expr, depth + 1)
            }
            ast::ExprPath(..) => {
                let mut i = 0;
                match self.def_map.borrow().find(&expr.id) {
                    None => fail!("path not found"),
                    Some(&df) => {
                        let mut def = df;
                        while i < depth {
                            match def {
                                def::DefUpvar(_, inner, _, _) => { def = *inner; }
                                _ => break
                            }
                            i += 1;
                        }
                        if i == depth { // Made it to end of loop
                            let dnum = def.def_id().node;
                            if !self.seen.contains(&dnum) {
                                self.refs.push(freevar_entry {
                                    def: def,
                                    span: expr.span,
                                });
                                self.seen.insert(dnum);
                            }
                        }
                    }
                }
            }
            _ => visit::walk_expr(self, expr, depth)
        }
    }


}

// Searches through part of the AST for all references to locals or
// upvars in this frame and returns the list of definition IDs thus found.
// Since we want to be able to collect upvars in some arbitrary piece
// of the AST, we take a walker function that we invoke with a visitor
// in order to start the search.
fn collect_freevars(def_map: &resolve::DefMap, blk: &ast::Block) -> Vec<freevar_entry> {
    let mut v = CollectFreevarsVisitor {
        seen: NodeSet::new(),
        refs: Vec::new(),
        def_map: def_map,
    };

    v.visit_block(blk, 1);
    v.refs
}

struct AnnotateFreevarsVisitor<'a> {
    def_map: &'a resolve::DefMap,
    freevars: freevar_map,
}

impl<'a> Visitor<()> for AnnotateFreevarsVisitor<'a> {
    fn visit_fn(&mut self, fk: &visit::FnKind, fd: &ast::FnDecl,
                blk: &ast::Block, s: Span, nid: ast::NodeId, _: ()) {
        let vars = collect_freevars(self.def_map, blk);
        self.freevars.insert(nid, vars);
        visit::walk_fn(self, fk, fd, blk, s, ());
    }
}

// Build a map from every function and for-each body to a set of the
// freevars contained in it. The implementation is not particularly
// efficient as it fully recomputes the free variables at every
// node of interest rather than building up the free variables in
// one pass. This could be improved upon if it turns out to matter.
pub fn annotate_freevars(def_map: &resolve::DefMap, krate: &ast::Crate)
                         -> freevar_map {
    let mut visitor = AnnotateFreevarsVisitor {
        def_map: def_map,
        freevars: NodeMap::new(),
    };
    visit::walk_crate(&mut visitor, krate, ());

    visitor.freevars
}

pub fn with_freevars<T>(tcx: &ty::ctxt, fid: ast::NodeId, f: |&[freevar_entry]| -> T) -> T {
    match tcx.freevars.borrow().find(&fid) {
        None => fail!("with_freevars: {} has no freevars", fid),
        Some(d) => f(d.as_slice())
    }
}

pub fn get_capture_mode(tcx: &ty::ctxt,
                        closure_expr_id: ast::NodeId)
                        -> CaptureMode
{
    let fn_ty = ty::node_id_to_type(tcx, closure_expr_id);
    match ty::ty_closure_store(fn_ty) {
        ty::RegionTraitStore(..) => CaptureByRef,
        ty::UniqTraitStore => CaptureByValue
    }
}
