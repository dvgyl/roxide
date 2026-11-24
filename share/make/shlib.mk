.DEFAULT_GOAL := all
roxide_config_shlib_ld := $(value SHLIB_LD)
CXX_STD ?= $(ROXIDE_DEFAULT_CXX_STD)

ALL_CPPFLAGS = $(R_XTRA_CPPFLAGS) $(CPPFLAGS) $(PKG_CPPFLAGS) $(CLINK_CPPFLAGS)
ALL_CFLAGS = $(CPICFLAGS) $(PKG_CFLAGS) $(CFLAGS) $(ROXIDE_LTO_C)
ALL_FFLAGS = $(FPICFLAGS) $(PKG_FFLAGS) $(FFLAGS) $(ROXIDE_LTO_F)
OBJC ?= $(CC)
OBJCXX ?= $(ROXIDE_CXX)
ifeq ($(origin F77),default)
F77 = $(FC)
endif
F77 ?= $(FC)

roxide_cxx_standard = $(if $(filter CXX11 CXX14 CXX17 CXX20 CXX23 CXX26,$(CXX_STD)),$(CXX_STD))
ROXIDE_CXX = $(if $($(roxide_cxx_standard)),$($(roxide_cxx_standard)) $($(roxide_cxx_standard)STD),$(CXX))
roxide_cxx_flags = $(if $(and $(roxide_cxx_standard),$(filter-out undefined,$(origin $(roxide_cxx_standard)$(1)))),$($(roxide_cxx_standard)$(1)),$(CXX$(1)))
ALL_CXXFLAGS = $(call roxide_cxx_flags,PICFLAGS) $(PKG_CXXFLAGS) $(call roxide_cxx_flags,FLAGS) $(ROXIDE_LTO_C)

ifeq ($(ROXIDE_USE_LTO),yes)
ROXIDE_LTO_C = $(LTO)
ROXIDE_LTO_F = $(LTO_FC)
ROXIDE_LTO_LINK = $(or $(LTO_LD),$(LTO))
endif

ROXIDE_COMPILE_c = $(CC) $(ALL_CPPFLAGS) $(ALL_CFLAGS) -c
ROXIDE_COMPILE_cpp = $(ROXIDE_CXX) $(ALL_CPPFLAGS) $(ALL_CXXFLAGS) -c
ROXIDE_COMPILE_cc = $(ROXIDE_COMPILE_cpp)
ROXIDE_COMPILE_cxx = $(ROXIDE_COMPILE_cpp)
ROXIDE_COMPILE_m = $(OBJC) $(ALL_CPPFLAGS) $(ALL_CFLAGS) $(OBJCFLAGS) $(PKG_OBJCFLAGS) -c
ROXIDE_COMPILE_mm = $(OBJCXX) $(ALL_CPPFLAGS) $(ALL_CXXFLAGS) $(OBJCXXFLAGS) $(PKG_OBJCFLAGS) -c
ROXIDE_COMPILE_f = $(F77) $(ALL_CPPFLAGS) $(ALL_FFLAGS) -c
ROXIDE_COMPILE_f90 = $(FC) $(ALL_CPPFLAGS) $(ALL_FFLAGS) $(FCFLAGS) $(PKG_FCFLAGS) -c
ROXIDE_COMPILE_f95 = $(ROXIDE_COMPILE_f90)

roxide_comma := ,
define roxide_rule
$(1): $(2)
	$$(ROXIDE_COMPILE_$(3)) "$$<" -o "$$@"
endef
$(foreach ext,c cc cpp cxx m mm f f90 f95,$(eval $(call roxide_rule,%.o,%.$(ext),$(ext))))
$(eval $(ROXIDE_SOURCE_RULES))

ifneq ($(ROXIDE_PACKAGE_MAKEVARS),)
include $(ROXIDE_PACKAGE_MAKEVARS)
endif
ifneq ($(ROXIDE_USER_MAKEVARS),)
include $(ROXIDE_USER_MAKEVARS)
endif

OBJECTS ?= $(ROXIDE_DEFAULT_OBJECTS)

.PHONY: all clean preclean
all: $(SHLIB)

roxide_ld = $(SHLIB_LD)
roxide_ldflags = $(SHLIB_LDFLAGS)
ifeq ($(ROXIDE_HAS_CXX),yes)
roxide_ld = $(or $(SHLIB_CXXLD),$(ROXIDE_CXX))
roxide_ldflags = $(SHLIB_CXXLDFLAGS)
ifneq ($(strip $($(roxide_cxx_standard))),)
ifeq ($(strip $(SHLIB_CXXLD)),$(strip $(CXX)))
roxide_ld = $(or $($(roxide_cxx_standard)LD),$(ROXIDE_CXX))
endif
endif
ifneq ($(roxide_config_shlib_ld),$(value SHLIB_LD))
roxide_ld = $(SHLIB_LD)
endif
ifeq ($(origin SHLIB_LD),command line)
roxide_ld = $(SHLIB_LD)
endif
endif

$(SHLIB): $(OBJECTS)
	$(roxide_ld) $(roxide_ldflags) $(ROXIDE_LTO_LINK) -o "$@" $(OBJECTS) $(PKG_LIBS) $(ROXIDE_LINK_FLAGS) $(LIBR)

RM ?= rm -f
ROXIDE_CLEAN_OBJECTS ?= $(OBJECTS)
clean preclean:
	$(RM) -- $(ROXIDE_CLEAN_OBJECTS)
