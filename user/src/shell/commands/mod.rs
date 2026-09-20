pub mod help;
pub mod pid;
pub mod run;
pub mod ps;
pub mod top;
pub mod jobs;
pub mod kill;
pub mod otto;
pub mod exit_cmd;
pub mod echo;
pub mod testmm1;
pub mod testsmp1;
pub mod testpar;
pub mod race;
pub mod ipi;
pub mod plog;
pub mod mmstat;
pub mod ipi_stat;
pub mod mmcompact;
pub mod mmmigrate;
pub mod rmapcount;
pub mod memstress;
pub mod testpmm2m;
pub mod time;
pub mod lspci;
pub mod pci_info;
pub mod test_pci;
pub mod driverhub;
pub mod ping;
pub mod getnet;
pub mod netinfo;
pub mod netdump;
pub mod dns;
pub mod netcfg;
pub mod whatis;
pub mod cat;
pub mod mkdir;
pub mod mv;
pub mod rm;
pub mod clear;
pub mod touch;
pub mod mount;
pub mod umount;
pub mod stat;
pub mod ls;

pub struct Command {
    pub name: &'static [u8],
    pub usage: &'static str,
    pub desc: &'static str,
    pub about: &'static str,
    pub run: fn(&[&[u8]]),
}

pub fn all_commands() -> &'static [Command] {
    &[
        help::CMD,
        pid::CMD,
        run::CMD,
        ps::CMD,
        top::CMD,
        jobs::CMD,
        kill::CMD,
        otto::CMD,
        exit_cmd::CMD,
        echo::CMD,
        testmm1::CMD,
        testsmp1::CMD,
        testpar::CMD,
        race::CMD,
        ipi::CMD,
        plog::CMD,
        mmstat::CMD,
        ipi_stat::CMD,
        mmcompact::CMD,
        mmmigrate::CMD,
        rmapcount::CMD,
        memstress::CMD,
        testpmm2m::CMD,
        time::CMD,
        lspci::CMD,
        pci_info::CMD,
        test_pci::CMD,
        driverhub::CMD,
        ping::CMD,
        getnet::CMD,
        netinfo::CMD,
        netdump::CMD,
        dns::CMD,
        netcfg::CMD,
        whatis::CMD,
        cat::CMD,
        mkdir::CMD,
        mv::CMD,
        rm::CMD,
        clear::CMD,
        touch::CMD,
        mount::CMD,
        umount::CMD,
        stat::CMD,
        ls::CMD,
    ]
}
