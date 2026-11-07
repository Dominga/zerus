from setuptools import Extension, setup
from Cython.Build import cythonize

setup(
    name="demo-cython",
    ext_modules=cythonize(
        [Extension("demo", ["src/demo.pyx"])],
        compiler_directives={"language_level": "3"},
    ),
)
