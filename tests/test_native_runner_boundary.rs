use orestra_conformance::native_runner::runner_environment;
#[test]
fn startup_injection_variables_are_dropped(){
 for key in ["BASH_ENV","PYTHONPATH","LD_PRELOAD","JAVA_TOOL_OPTIONS"]{std::env::set_var(key,"unsafe");}
 let env=runner_environment();for key in ["BASH_ENV","PYTHONPATH","LD_PRELOAD","JAVA_TOOL_OPTIONS"]{assert!(!env.contains_key(key));}
 assert_eq!(env["ORESTRA_SOURCE_TEST"],"1");
}
