use orestra_conformance::cluster::Cluster;

fn converge(c:&mut Cluster,epoch:u64){c.reconcile(epoch).unwrap();c.start_assigned();c.assert_invariants().unwrap();}

#[test]
fn replica_reconciliation_idempotent(){
    let mut c=Cluster::new();c.add_node("n1",1000,2000).unwrap();c.add_node("n2",1000,2000).unwrap();c.deploy("api",3,400,500).unwrap();
    let e=c.takeover();converge(&mut c,e);assert_eq!(c.active(Some("api")).len(),3);
    let prior:Vec<_>=c.active(None).iter().map(|i|i.uid).collect();converge(&mut c,e);
    assert_eq!(c.active(None).iter().map(|i|i.uid).collect::<Vec<_>>(),prior);
}
#[test]
fn assignment_waits_for_agent_ack(){
    let mut c=Cluster::new();c.add_node("n1",1000,1000).unwrap();c.deploy("api",1,100,100).unwrap();let e=c.takeover();c.reconcile(e).unwrap();
    assert_eq!(c.active(Some("api")).len(),0);assert_eq!(c.assigned(Some("api")).len(),1);let uid=c.assigned(Some("api"))[0].uid;
    assert!(c.agent_start(uid,1,false).is_err());assert_eq!(c.usage("n1"),(100,100));c.agent_start(uid,1,true).unwrap();assert_eq!(c.active(Some("api")).len(),1);
}
#[test]
fn stale_agent_epoch_cannot_ack(){
    let mut c=Cluster::new();c.add_node("n1",1000,1000).unwrap();c.deploy("api",1,100,100).unwrap();let e=c.takeover();c.reconcile(e).unwrap();let uid=c.assigned(Some("api"))[0].uid;
    assert_eq!(c.restart_agent("n1").unwrap(),2);assert_eq!(c.instances[&uid].phase,"Unknown");assert!(c.agent_start(uid,1,true).is_err());
    assert_eq!(c.usage("n1"),(100,100));c.agent_confirm_absent(uid,2).unwrap();assert_eq!(c.usage("n1"),(0,0));converge(&mut c,e);assert_eq!(c.active(Some("api")).len(),1);
}
#[test]
fn fenced_epoch_and_node_loss(){
    let mut c=Cluster::new();c.add_node("a",400,400).unwrap();c.add_node("b",400,400).unwrap();c.deploy("web",2,300,300).unwrap();
    let old=c.takeover();let current=c.takeover();assert!(c.reconcile(old).is_err());converge(&mut c,current);assert_eq!(c.active(None).len(),2);
    c.set_ready("a",false).unwrap();converge(&mut c,current);assert_eq!(c.active(None).len(),1);assert_eq!(c.instances.values().filter(|i|i.phase=="Unknown").count(),1);
}
#[test]
fn capacity_scale_down_and_stop_proof(){
    let mut c=Cluster::new();c.add_node("node",1000,1000).unwrap();c.deploy("a",10,200,200).unwrap();let e=c.takeover();converge(&mut c,e);assert_eq!(c.active(None).len(),5);
    c.deploy("a",2,200,200).unwrap();c.reconcile(e).unwrap();assert_eq!(c.active(None).len(),2);assert_eq!(c.instances.values().filter(|i|i.phase=="Terminating").count(),3);
    let uid=c.instances.values().find(|i|i.phase=="Terminating").unwrap().uid;assert!(c.confirm_stopped(uid,false).is_err());c.confirm_stopped(uid,true).unwrap();
}
#[test]
fn external_fence_required_before_replacement(){
    let mut c=Cluster::new();for n in ["a","b","c"]{c.add_node(n,500,500).unwrap();}c.deploy("web",2,400,400).unwrap();let e=c.takeover();converge(&mut c,e);
    let initial:Vec<_>=c.active(Some("web")).iter().map(|i|i.node.clone()).collect();let missing=initial.iter().min().unwrap().clone();c.set_ready(&missing,false).unwrap();converge(&mut c,e);
    assert_eq!(c.active(Some("web")).len(),1);assert!(c.confirm_fenced(&missing,false).is_err());assert_eq!(c.confirm_fenced(&missing,true).unwrap(),1);converge(&mut c,e);
    assert_eq!(c.active(Some("web")).len(),2);assert!(!c.active(Some("web")).iter().any(|i|i.node==missing));
}
#[test]
fn node_rejoin_does_not_reanimate_or_free_unknown(){
    let mut c=Cluster::new();c.add_node("n",500,500).unwrap();c.deploy("a",1,500,500).unwrap();let e=c.takeover();converge(&mut c,e);
    c.set_ready("n",false).unwrap();c.set_ready("n",true).unwrap();c.deploy("b",1,500,500).unwrap();converge(&mut c,e);
    assert_eq!(c.active(Some("b")).len(),0);assert_eq!(c.usage("n"),(500,500));assert!(c.confirm_fenced("n",true).is_err());
}
struct Rng(u64);impl Rng{fn next(&mut self,n:u64)->u64{self.0^=self.0<<13;self.0^=self.0>>7;self.0^=self.0<<17;self.0%n}}
#[test]
fn deterministic_500_step_fault_campaign(){
    let mut rng=Rng(1042);let mut c=Cluster::new();for i in 0..5{c.add_node(&format!("node-{i}"),2000,4000).unwrap();}
    c.deploy("api",5,200,300).unwrap();c.deploy("worker",3,400,700).unwrap();let mut e=c.takeover();
    for _ in 0..500{let action=rng.next(6);let node=format!("node-{}",rng.next(5));match action{
        0=>{c.set_ready(&node,false).unwrap();},1=>{c.set_ready(&node,true).unwrap();},2=>{c.deploy("api",rng.next(9) as usize,200,300).unwrap();},
        3=>{c.deploy("worker",rng.next(7) as usize,400,700).unwrap();},4=>{e=c.takeover();},_=>{c.restart_agent(&node).unwrap();}
    }c.reconcile(e).unwrap();c.start_assigned();c.assert_invariants().unwrap();}
}
