//!
//! Virtual Machine module. Implements all atom types and related stuff. Should be
//! optimized for speed. There are many virtual machines in a world at the same time.
//! One VM runs one molecule.
//! 
pub mod buf;
pub mod vmdata;

use crate::Core;
use crate::global::Atom;
use crate::global::{*};
use crate::atom::{*};
//
// map between atom type number and handler fn index. Should be in stack
//
const ATOMS_MAP: &[fn(&mut VM, Atom, &mut Core) -> bool] = &[
    VM::atom_empty,  // 0 - must be an empty fn. Means empty cell or no atom
    VM::atom_mov,
    VM::atom_fix,
    VM::atom_spl,
    VM::atom_if,
    VM::atom_job,
    VM::atom_empty,  // unused
    VM::atom_empty   // unused
];
///
/// Index of if atom. Must be synchronized with ATOMS_MAP
///
const ATOM_IF: Atom = 4;
///
/// Describes data for one instance of Virtual Machine
///
#[derive(Copy, Clone)]
pub struct VM {
    ///
    /// Energy of current VM. Every VM may have it's own.
    ///
    energy: isize,
    ///
    /// Offset of current atom, which VM in running.
    ///
    offs: Offs
}

impl VM {
    pub fn new(energy: isize, offs: Offs) -> VM {
        VM {
            energy,
            offs
        }
    }
    ///
    /// Runs one atom depending on type and moves VM to the next one depending on
    /// atom direction.
    ///
    pub fn run_atom(&mut self, core: &mut Core) -> bool {
        let atom: Atom = core.vm_data.world.get_atom(self.offs);
        let atom_type = get_type(atom);
        if atom_type == ATOM_EMPTY { return false }

        ATOMS_MAP[atom_type as I](self, atom, core)
    }
    ///
    /// Returns energy amount of current VM
    ///
    pub fn get_energy(&self) -> isize { self.energy }
    ///
    /// Returns offset of current VM
    ///
    pub fn get_offs(&self) -> Offs { self.offs }
    ///
    /// Implements mov command. It moves current atom and all binded atoms together.
    /// Should be optimized by speed. After moving all bonds should not be broken.
    ///
    fn atom_mov(&mut self, mut atom: Atom, core: &mut Core) -> bool {
        let mut offs: Offs;
        let mut to_offs: Offs;
        let mut d0: Dir;
        let mut d1: Dir;
        let mut a: Atom;
        let mut o: Offs;
        let dir = get_dir1(atom);                                         // atom move direction
        let stack = &mut core.vm_data.buf.stack;
        let wrld  = &mut core.vm_data.world;
        let buf   = &mut core.vm_data.buf.buf;
        let mov_energy = core.cfg.atoms().mov_energy;

        stack.clear();                                                    // every call of mov should reset stack & buf
        buf.clear();
        stack.push(self.offs);

        while !stack.empty() {                                            // before while, stack should have >= 1 atom
            offs = stack.last().unwrap();                                 // offset of atom before move
            if buf.contains(&offs) { stack.shrink(); continue }           // this atom was already moved
            atom = wrld.get_atom(offs);                                   // atom we have to move
            to_offs = wrld.get_offs(offs, dir);                           // destination atom position
            if wrld.is_atom(to_offs) {                                    // can't move atom. Another one is there
                stack.push(to_offs);
                continue;
            }
            stack.shrink();                                               // destination cell is empty, can move there
            wrld.mov_atom(offs, to_offs, &core.io);                       // move atom physically
            buf.insert(to_offs);                                          // mark atom as "already moved"
            self.energy -= mov_energy;                                    // decrease energy for every moved atom
            // update vm bond of moved atom---------------------------------------------------------------------------------
            d0 = get_vm_dir(atom);                                        // get VM dir of moved atom
            d1 = DIR_MOV_ATOM[d0 as I][dir as I];                         // final dir of moved atom
            o  = wrld.get_offs(offs, d0);                                 // offs of near atom
            if d1 == DIR_NO { stack.push(o); }                            // near atom is to far, will add it later
            else {
                set_vm_dir(&mut atom, d1);                            // distance between atoms is 1. update bond
                set_vm_bond(&mut atom);
                wrld.set_atom(to_offs, atom, &core.io);
                // update vm bond of near atom------------------------------------------------------------------------------
                d0 = DIR_REV[d0 as I];                                    // get near atom's dir to moved atom
                a  = wrld.get_atom(o);                               // near atom
                if get_vm_dir(a) == d0 {                             // near atom has a bond with moved
                    d1 = DIR_NEAR_ATOM[d0 as I][dir as I];                // final dir of near atom
                    set_vm_dir(&mut a, d1);
                    set_vm_bond(&mut a);
                    wrld.set_atom(o, a, &core.io);
                }
            }

            if get_type(atom) == ATOM_IF {                                // if atom has additional then bond
                // update then bond of moved atom---------------------------------------------------------------------------
                d0 = get_dir2(atom);                                      // get then dir of if moved atom
                d1 = DIR_MOV_ATOM[d0 as I][dir as I];                     // final dir of if moved atom
                o  = wrld.get_offs(offs, d0);                         // offs of near atom
                if d1 == DIR_NO { stack.push(o); }                        // near atom is to far, will add it later
                else {
                    set_dir2(&mut atom, d1);                          // distance between atoms is 1. update bond
                    wrld.set_atom(to_offs, atom, &core.io);
                    // update then bond of near atom------------------------------------------------------------------------
                    d0 = DIR_REV[d0 as I];                                // get near atom's dir to moved atom
                    a  = wrld.get_atom(o);                           // near atom
                    if get_dir2(a) == d0 {                           // near atom has a bond with moved
                        d1 = DIR_NEAR_ATOM[d0 as I][dir as I];            // final dir of near atom
                        set_dir2(&mut a, d1);
                        wrld.set_atom(o, a, &core.io);
                    }
                }
            }
        }
        if has_vm_bond(atom) {
            self.offs = wrld.get_offs(self.offs, get_vm_dir(atom));
        }

        true
    }
    ///
    /// Implements fix atom. Creates vm bond between two atoms. If vm bond is already exist, than
    /// try to create if/then bond for if atom. Consumes energy.
    ///
    fn atom_fix(&mut self, atom: Atom, core: &mut Core) -> bool {
        let offs0 = core.vm_data.world.get_offs(self.offs, get_dir1(atom)); // gets first near atom offs to fix
        let mut atom0 = core.vm_data.world.get_atom(offs0);               // gets first near atom to fix
        if !is_atom(atom0) { return false }                               // no first near atom to fix
        let d0 = get_dir2(atom);
        if !is_atom(core.vm_data.world.get_dir_atom(offs0, d0)) { return false } // there is no second near atom to fix

        // fix vm bond------------------------------------------------------------------------------------------------------
        if !has_vm_bond(atom0) {                                          // first near atom has no vm bond
            set_vm_dir(&mut atom0, d0);
            set_vm_bond(&mut atom0);
            core.vm_data.world.set_atom(offs0, atom0, &core.io);
            if has_vm_bond(atom) { self.offs = core.vm_data.world.get_offs(self.offs, get_vm_dir(atom)) }
            self.energy -= core.cfg.atoms().fix_energy;
            return true;
        }
        if get_type(atom0) != ATOM_IF { return false }                    // only if atom has if and then bonds
        // fix then bond----------------------------------------------------------------------------------------------------
        if !has_dir2_bond(atom0) {                                       // first near atom has no then bond
            set_dir2(&mut atom0, d0);
            set_dir2_bond(&mut atom0);
            core.vm_data.world.set_atom(offs0, atom0, &core.io);
            if has_vm_bond(atom) { self.offs = core.vm_data.world.get_offs(self.offs, get_vm_dir(atom)) }
            self.energy -= core.cfg.atoms().fix_energy;
            return true;
        }

        false
    }
    ///
    /// Implements spl atom. Splits two atoms bonds. If atoms has no vm bond, than
    /// try to split if/then bonds for if atom. Releases energy.
    ///
    fn atom_spl(&mut self, atom: Atom, core: &mut Core) -> bool {
        let offs0 = core.vm_data.world.get_offs(self.offs, get_dir1(atom)); // gets first near atom offs to split
        let mut atom0 = core.vm_data.world.get_atom(offs0);               // gets first near atom to split
        if !is_atom(atom0) { return false }                               // no first near atom to split
        let d0 = get_dir2(atom);
        if !is_atom(core.vm_data.world.get_dir_atom(offs0, d0)) { return false }  // there is no second near atom to split

        // split vm bond----------------------------------------------------------------------------------------------------
        if has_vm_bond(atom0) {                                           // first near atom has vm bond
            reset_vm_bond(&mut atom0);
            core.vm_data.world.set_atom(offs0, atom0, &core.io);
            if has_vm_bond(atom) { self.offs = core.vm_data.world.get_offs(self.offs, get_vm_dir(atom)) }
            self.energy += core.cfg.atoms().spl_energy;
            return true;
        }
        if get_type(atom0) != ATOM_IF { return false }
        // split then bond--------------------------------------------------------------------------------------------------
        if has_dir2_bond(atom0) {                                         // first near atom has then bond
            reset_dir2_bond(&mut atom0);
            core.vm_data.world.set_atom(offs0, atom0, &core.io);
            if has_vm_bond(atom) { self.offs = core.vm_data.world.get_offs(self.offs, get_vm_dir(atom)) }
            self.energy += core.cfg.atoms().spl_energy;
            return true;
        }

        false
    }
    ///
    /// Implements cond command. Depending on the condition VM will run one of two
    /// possible atoms.
    ///
    fn atom_if(&mut self, atom: Atom, core: &mut Core) -> bool {
        // runs if -> then scenario
        if has_dir2_bond(atom) && is_atom(core.vm_data.world.get_dir_atom(self.offs, get_dir1(atom))) {
            self.offs = core.vm_data.world.get_offs(self.offs, get_dir2(atom));
            self.energy -= core.cfg.atoms().if_energy;
            return true;
        }
        // runs else scenario
        if has_vm_bond(atom) {
            self.offs = core.vm_data.world.get_offs(self.offs, get_vm_dir(atom));
            self.energy -= core.cfg.atoms().if_energy;
            return true;
        }

        false
    }
    ///
    /// Implements job command. Creates one new VM instance (thread). Energy decreasing
    /// should be called from outside, because new VM is added there
    ///
    fn atom_job(&mut self, atom: Atom, core: &mut Core) -> bool {
        let offs = core.vm_data.world.get_offs(self.offs, get_vm_dir(atom));
        if !is_atom(core.vm_data.world.get_atom(offs)) { return false }
        let energy = self.energy / 2;
        self.energy -= energy;
        if !core.vms.full() {
            core.vms.add(VM::new(energy, offs));
            return true;
        }

        false
    }
    ///
    /// Just a stub for empty atom in a world
    ///
    fn atom_empty(&mut self, _atom: Atom, _core: &mut Core) -> bool { false }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};
    use crate::{cfg::Config, Core, io::IO, vm::vmdata::VMData};
    use crate::utils::{id, vec::Vector};

    use super::VM;

    fn create_file(file: &str, content: &str) {
        assert_eq!(fs::write(file, content).is_ok(), true);
    }
    fn remove_file(file: &str) {
        if Path::new(file).exists() { assert_eq!(fs::remove_file(file).is_ok(), true) }
    }
    #[test]
    fn test_new() {
        let cfg_file = id() + ".json";
        create_file(&cfg_file, r#"{"WIDTH": 10, "HEIGHT": 10, "MAX_VM_AMOUNT": 1}"#);

        let cfg = Config::new(&cfg_file);
        let vm_amount = cfg.MAX_VM_AMOUNT();
        let width = cfg.WIDTH();
        let height = cfg.HEIGHT();
        let dir2offs = cfg.DIR_TO_OFFS();
        let mov_buf_size = cfg.MOV_BUF_SIZE();
        let core = Box::into_raw(Box::new(Core {
            cfg,
            vms: Vector::new(vm_amount),
            io: IO::new(),
            vm_data: VMData::new(width, height, dir2offs, mov_buf_size)
        })).cast();
        let pvms = unsafe{ &mut (*(core as *mut Core)).vms };
        let pvmdata = unsafe{ &mut (*(core as *mut Core)).vm_data };
        let pio = unsafe{ &mut (*(core as *mut Core)).io };
        let pcore = unsafe{ &mut *(core as *mut Core) };

        pvms.add(VM::new(100, 0));
        pvmdata.world.set_atom(0, 0b0010_0000_1100_0000, pio); // atom: mov right
        assert_eq!(pvmdata.world.get_atom(0), 0b0010_0000_1100_0000);
        pvms.data[0].run_atom(pcore);
        assert_eq!(pvmdata.world.get_atom(1), 0b0010_0000_1100_0000);

        remove_file(&cfg_file);
    }
}