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

ROXIDE_CXX = $(if $(CXX_STD),$($(CXX_STD)) $($(CXX_STD)STD),$(CXX))
roxide_cxx_flags = $(if $(and $(CXX_STD),$(filter-out undefined,$(origin $(CXX_STD)$(1)))),$($(CXX_STD)$(1)),$(CXX$(1)))
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
roxide_source_extensions := c cc cpp cxx m mm f f90 f95
$(foreach ext,$(roxide_source_extensions),$(eval $(call roxide_rule,%.o,%.$(ext),$(ext))))
$(eval $(ROXIDE_SOURCE_RULES))

ifneq ($(ROXIDE_PACKAGE_MAKEVARS),)
include $(ROXIDE_PACKAGE_MAKEVARS)
endif
ifneq ($(ROXIDE_USER_MAKEVARS),)
include $(ROXIDE_USER_MAKEVARS)
endif

OBJECTS ?= $(ROXIDE_DEFAULT_OBJECTS)

ifneq ($(strip $(CXX_STD)),)
ifeq ($(and $(filter CXX11 CXX14 CXX17 CXX20 CXX23 CXX26,$(CXX_STD)),$(strip $($(CXX_STD)))),)
$(error unsupported CXX_STD '$(CXX_STD)' or empty compiler variable)
endif
endif

ifneq ($(ROXIDE_PREBUILT_OBJECTS),)
$(ROXIDE_PREBUILT_OBJECTS): ;
endif

define roxide_object_kind
roxide_object_kind() { \
	for input in $(ROXIDE_PREBUILT_OBJECTS); do \
		if [ "$$1" = "$$input" ] || [ "$$1" -ef "$$input" ]; then return; fi; \
	done; \
	for input in $(ROXIDE_SOURCE_OBJECTS); do \
		if [ "$$1" = "$$input" ]; then \
			for cxx in $(ROXIDE_CXX_OBJECTS); do \
				if [ "$$1" = "$$cxx" ]; then echo cxx; return; fi; \
			done; \
			echo c; return; \
		fi; \
	done; \
	case "$$1" in *.o) \
		for ext in $(roxide_source_extensions); do \
			if [ -f "$${1%.o}.$$ext" ]; then echo "$$ext"; return; fi; \
		done ;; \
	esac; \
}
endef

.PHONY: all clean preclean
all: $(SHLIB)

roxide_object_kinds = $(shell $(roxide_object_kind); for object in $(OBJECTS); do roxide_object_kind "$$object"; done)
roxide_cxx_ld = $(or $(SHLIB_CXXLD),$(ROXIDE_CXX))
roxide_cxx_ldflags = $(or $(SHLIB_CXXLDFLAGS),$(SHLIB_LDFLAGS))
ifneq ($(strip $(CXX_STD)),)
ifeq ($(strip $(SHLIB_CXXLD)),$(strip $(CXX)))
roxide_cxx_ld = $(or $($(CXX_STD)LD),$(ROXIDE_CXX))
endif
endif
ifneq ($(roxide_config_shlib_ld),$(value SHLIB_LD))
roxide_cxx_ld = $(SHLIB_LD)
endif
ifeq ($(origin SHLIB_LD),command line)
roxide_cxx_ld = $(SHLIB_LD)
endif

roxide_link = $(if $(filter cc cpp cxx mm,$(roxide_object_kinds)),$(roxide_cxx_ld) $(roxide_cxx_ldflags),$(SHLIB_LD) $(SHLIB_LDFLAGS))

$(SHLIB): $(OBJECTS)
	$(roxide_link) $(ROXIDE_LTO_LINK) -o "$@" $(OBJECTS) $(PKG_LIBS) $(ROXIDE_LINK_FLAGS) $(LIBR)

RM ?= rm -f
clean preclean:
	@$(roxide_object_kind); set --; \
	for object in $(OBJECTS); do \
		if [ -n "$$(roxide_object_kind "$$object")" ]; then set -- "$$@" "$$object"; fi; \
	done; \
	if [ "$$#" -gt 0 ]; then $(RM) -- "$$@"; fi
