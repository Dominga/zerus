C++ builds and tests of any project (hgs guide team-cpp)

hgs team run templates, typed like the others (values in your folder):
  cmake-configure {source?, build_dir?, generator?, config?, preset?}
                             any CMake project; default . into build
  cmake-build {build_dir?, config?, targets?}  cmake --build
  ctest {build_dir?, config?, tests? or regex?}  one result per test
  msbuild-build {project, config?, platform?, targets?}
                             a .sln or project file, Visual Studio's MSBuild
  vstest {tests, filter?}    test assemblies; one result per test
On Windows they load Visual Studio; never wrap them in your own vcvars.
A test run's answer carries "report": each test's name, status, time and
first message, failures first. The exit code stays the verdict.
Task #58: the Qt desktop's qt-configure, qt-build and qt-test are gone; use these.
A project's .zerus/tests.toml may declare its own [[template]] entries with
typed params; each runs only once the owner accepted its exact content.
Anything else still asks the lead.
