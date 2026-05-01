package main

import (
	"encoding/json"
	"go/ast"
	"go/parser"
	"go/token"
	"go/types"
	"io"
	"os"
	"os/exec"
	"path"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
)

type helperOutput struct {
	Types          []typeMembers  `json:"types"`
	Heritage       []typeHeritage `json:"heritage"`
	MemberAccesses []memberAccess `json:"member_accesses"`
}

type typeMembers struct {
	Name    string         `json:"name"`
	Members []helperMember `json:"members"`
}

type helperMember struct {
	Name  string `json:"name"`
	Kind  string `json:"kind"`
	Start int    `json:"start"`
	End   int    `json:"end"`
}

type typeHeritage struct {
	ExportName string   `json:"export_name"`
	Implements []string `json:"implements"`
}

type memberAccess struct {
	Object string `json:"object"`
	Member string `json:"member"`
}

func main() {
	path := "input.go"
	if len(os.Args) > 1 {
		path = os.Args[1]
	}

	source, err := io.ReadAll(os.Stdin)
	if err != nil {
		os.Exit(1)
	}

	fset := token.NewFileSet()
	file, err := parser.ParseFile(fset, path, source, parser.SkipObjectResolution)
	if err != nil {
		os.Exit(1)
	}

	membersByType := map[string]map[string]helperMember{}
	importLocals := collectImportLocals(file)
	exportedTypes := map[string]struct{}{}

	for _, decl := range file.Decls {
		switch decl := decl.(type) {
		case *ast.GenDecl:
			if decl.Tok != token.TYPE {
				continue
			}
			for _, spec := range decl.Specs {
				typeSpec, ok := spec.(*ast.TypeSpec)
				if !ok || !typeSpec.Name.IsExported() {
					continue
				}
				typeName := typeSpec.Name.Name
				exportedTypes[typeName] = struct{}{}
				switch t := typeSpec.Type.(type) {
				case *ast.StructType:
					for _, field := range t.Fields.List {
						for _, name := range field.Names {
							if name.IsExported() {
								addMember(
									membersByType,
									fset,
									typeName,
									name.Name,
									"property",
									name.Pos(),
									name.End(),
								)
							}
						}
					}
				case *ast.InterfaceType:
					for _, field := range t.Methods.List {
						for _, name := range field.Names {
							if name.IsExported() {
								addMember(
									membersByType,
									fset,
									typeName,
									name.Name,
									"method",
									name.Pos(),
									name.End(),
								)
							}
						}
					}
				}
			}
		case *ast.FuncDecl:
			if decl.Recv == nil || decl.Name == nil || !decl.Name.IsExported() {
				continue
			}
			recvName := receiverTypeName(decl.Recv.List)
			if recvName == "" || !ast.IsExported(recvName) {
				continue
			}
			addMember(
				membersByType,
				fset,
				recvName,
				decl.Name.Name,
				"method",
				decl.Name.Pos(),
				decl.Name.End(),
			)
		}
	}

	helperTargets := collectHelperTargets(path, file, importLocals, exportedTypes)
	accesses := collectMemberAccesses(file, importLocals, exportedTypes, helperTargets)
	accesses = append(
		accesses,
		collectTypedMemberAccesses(path, source, importLocals, exportedTypes, helperTargets)...,
	)
	accesses = dedupeMemberAccesses(accesses)

	output := helperOutput{
		Types:          []typeMembers{},
		Heritage:       []typeHeritage{},
		MemberAccesses: []memberAccess{},
	}
	for typeName, members := range membersByType {
		entry := typeMembers{Name: typeName, Members: []helperMember{}}
		for _, member := range members {
			entry.Members = append(entry.Members, member)
		}
		sort.Slice(entry.Members, func(i, j int) bool {
			if entry.Members[i].Name == entry.Members[j].Name {
				return entry.Members[i].Kind < entry.Members[j].Kind
			}
			return entry.Members[i].Name < entry.Members[j].Name
		})
		output.Types = append(output.Types, entry)
	}
	sort.Slice(output.Types, func(i, j int) bool {
		return output.Types[i].Name < output.Types[j].Name
	})
	output.Heritage = collectImplementedInterfaces(path, source)
	output.MemberAccesses = accesses
	if output.Heritage == nil {
		output.Heritage = []typeHeritage{}
	}
	if output.MemberAccesses == nil {
		output.MemberAccesses = []memberAccess{}
	}

	encoder := json.NewEncoder(os.Stdout)
	if err := encoder.Encode(output); err != nil {
		os.Exit(1)
	}
}

func collectImportLocals(file *ast.File) map[string]struct{} {
	locals := map[string]struct{}{}
	for _, spec := range file.Imports {
		if spec.Name != nil {
			if spec.Name.Name == "_" || spec.Name.Name == "." {
				continue
			}
			locals[spec.Name.Name] = struct{}{}
			continue
		}
		importPath := strings.Trim(spec.Path.Value, "\"")
		locals[path.Base(importPath)] = struct{}{}
	}
	return locals
}

func collectImportAliases(file *ast.File) map[string]string {
	aliases := map[string]string{}
	for _, spec := range file.Imports {
		importPath := strings.Trim(spec.Path.Value, "\"")
		if spec.Name != nil {
			if spec.Name.Name == "_" || spec.Name.Name == "." {
				continue
			}
			aliases[importPath] = spec.Name.Name
			continue
		}
		aliases[importPath] = path.Base(importPath)
	}
	return aliases
}

func collectMemberAccesses(
	file *ast.File,
	importLocals map[string]struct{},
	exportedTypes map[string]struct{},
	helperTargets helperTargets,
) []memberAccess {
	accesses := []memberAccess{}
	seen := map[string]struct{}{}

	for _, decl := range file.Decls {
		funcDecl, ok := decl.(*ast.FuncDecl)
		if !ok || funcDecl.Body == nil {
			continue
		}
		processBlock(
			funcDecl.Body.List,
			map[string]string{},
			importLocals,
			exportedTypes,
			helperTargets,
			seen,
			&accesses,
		)
	}

	sort.Slice(accesses, func(i, j int) bool {
		if accesses[i].Object == accesses[j].Object {
			return accesses[i].Member < accesses[j].Member
		}
		return accesses[i].Object < accesses[j].Object
	})
	return accesses
}

func collectTypedMemberAccesses(
	currentPath string,
	source []byte,
	importLocals map[string]struct{},
	exportedTypes map[string]struct{},
	helperTargets helperTargets,
) []memberAccess {
	fset := token.NewFileSet()
	files, primary := parseFilesForTypeChecking(fset, currentPath, source)
	if len(files) == 0 || primary == nil {
		return nil
	}
	importAliases := collectImportAliases(primary)

	info := &types.Info{
		Defs:       map[*ast.Ident]types.Object{},
		Uses:       map[*ast.Ident]types.Object{},
		Types:      map[ast.Expr]types.TypeAndValue{},
		Selections: map[*ast.SelectorExpr]*types.Selection{},
	}
	importer := newSourceImporter(fset, currentPath)
	conf := types.Config{
		Importer: importer,
		Error:    func(error) {},
	}
	pkg, _ := conf.Check(primary.Name.Name, fset, files, info)
	if pkg == nil {
		return nil
	}

	paramSpecializations := collectParamConcreteSpecializations(
		files,
		pkg,
		info,
		importAliases,
		importLocals,
		exportedTypes,
		helperTargets,
	)
	accesses := []memberAccess{}
	seen := map[string]struct{}{}
	emptyBindings := map[string]string{}
	ast.Inspect(primary, func(node ast.Node) bool {
		selector, ok := node.(*ast.SelectorExpr)
		if !ok || !selector.Sel.IsExported() {
			return true
		}
		if specializedTargets, ok := paramSpecializations[selector]; ok {
			for _, target := range specializedTargets {
				key := target + "." + selector.Sel.Name
				if _, exists := seen[key]; exists {
					continue
				}
				seen[key] = struct{}{}
				accesses = append(accesses, memberAccess{
					Object: target,
					Member: selector.Sel.Name,
				})
			}
			return true
		}
		if target := resolveExprTarget(
			selector.X,
			emptyBindings,
			importLocals,
			exportedTypes,
			helperTargets,
		); target != "" {
			return true
		}
		selection := info.Selections[selector]
		if selection == nil {
			return true
		}
		target := namedTypeTargetFromTypes(selection.Recv(), pkg, importAliases)
		if target == "" {
			if typeAndValue, ok := info.Types[selector.X]; ok {
				target = namedTypeTargetFromTypes(typeAndValue.Type, pkg, importAliases)
			}
		}
		if target == "" {
			return true
		}
		key := target + "." + selector.Sel.Name
		if _, exists := seen[key]; exists {
			return true
		}
		seen[key] = struct{}{}
		accesses = append(accesses, memberAccess{
			Object: target,
			Member: selector.Sel.Name,
		})
		return true
	})
	return accesses
}

type paramMemberKey struct {
	funcName   string
	paramIndex int
	member     string
}

type paramMemberSpecialization struct {
	targets    map[string]struct{}
	callCount  int
	unresolved bool
}

func collectParamConcreteSpecializations(
	files []*ast.File,
	currentPkg *types.Package,
	info *types.Info,
	importAliases map[string]string,
	importLocals map[string]struct{},
	exportedTypes map[string]struct{},
	helperTargets helperTargets,
) map[*ast.SelectorExpr][]string {
	paramMembersByFunc := map[string]map[int]map[string]struct{}{}
	selectorKeys := map[*ast.SelectorExpr]paramMemberKey{}

	for _, file := range files {
		for _, decl := range file.Decls {
			funcDecl, ok := decl.(*ast.FuncDecl)
			if !ok || funcDecl.Recv != nil || funcDecl.Body == nil || funcDecl.Name == nil {
				continue
			}
			if ast.IsExported(funcDecl.Name.Name) {
				continue
			}
			paramIndices := map[types.Object]int{}
			if funcDecl.Type.Params != nil {
				index := 0
				for _, field := range funcDecl.Type.Params.List {
					for _, name := range field.Names {
						if obj := info.Defs[name]; obj != nil {
							paramIndices[obj] = index
						}
						index++
					}
				}
			}
			if len(paramIndices) == 0 {
				continue
			}
			ast.Inspect(funcDecl.Body, func(node ast.Node) bool {
				selector, ok := node.(*ast.SelectorExpr)
				if !ok || !selector.Sel.IsExported() {
					return true
				}
				ident, ok := selector.X.(*ast.Ident)
				if !ok {
					return true
				}
				paramObj := info.Uses[ident]
				paramIndex, ok := paramIndices[paramObj]
				if !ok {
					return true
				}
				key := paramMemberKey{
					funcName:   funcDecl.Name.Name,
					paramIndex: paramIndex,
					member:     selector.Sel.Name,
				}
				if _, ok := paramMembersByFunc[key.funcName]; !ok {
					paramMembersByFunc[key.funcName] = map[int]map[string]struct{}{}
				}
				if _, ok := paramMembersByFunc[key.funcName][key.paramIndex]; !ok {
					paramMembersByFunc[key.funcName][key.paramIndex] = map[string]struct{}{}
				}
				paramMembersByFunc[key.funcName][key.paramIndex][key.member] = struct{}{}
				selectorKeys[selector] = key
				return true
			})
		}
	}

	if len(selectorKeys) == 0 {
		return nil
	}

	specializations := map[paramMemberKey]*paramMemberSpecialization{}
	for _, file := range files {
		for _, decl := range file.Decls {
			funcDecl, ok := decl.(*ast.FuncDecl)
			if !ok || funcDecl.Body == nil {
				continue
			}
			collectCallSiteParamSpecializations(
				funcDecl.Body.List,
				map[string]string{},
				currentPkg,
				info,
				importAliases,
				importLocals,
				exportedTypes,
				helperTargets,
				paramMembersByFunc,
				specializations,
			)
		}
	}

	resolved := map[*ast.SelectorExpr][]string{}
	for selector, key := range selectorKeys {
		spec := specializations[key]
		if spec == nil || spec.unresolved || spec.callCount == 0 || len(spec.targets) == 0 {
			continue
		}
		targets := make([]string, 0, len(spec.targets))
		for target := range spec.targets {
			targets = append(targets, target)
		}
		sort.Strings(targets)
		resolved[selector] = targets
	}
	return resolved
}

func collectCallSiteParamSpecializations(
	stmts []ast.Stmt,
	bindings map[string]string,
	currentPkg *types.Package,
	info *types.Info,
	importAliases map[string]string,
	importLocals map[string]struct{},
	exportedTypes map[string]struct{},
	helperTargets helperTargets,
	paramMembersByFunc map[string]map[int]map[string]struct{},
	specializations map[paramMemberKey]*paramMemberSpecialization,
) {
	for _, stmt := range stmts {
		switch stmt := stmt.(type) {
		case *ast.DeclStmt:
			genDecl, ok := stmt.Decl.(*ast.GenDecl)
			if ok && genDecl.Tok == token.VAR {
				for _, spec := range genDecl.Specs {
					valueSpec, ok := spec.(*ast.ValueSpec)
					if !ok {
						continue
					}
					for _, value := range valueSpec.Values {
						collectCallSiteParamSpecializationsInExpr(
							value,
							bindings,
							currentPkg,
							info,
							importAliases,
							importLocals,
							exportedTypes,
							helperTargets,
							paramMembersByFunc,
							specializations,
						)
					}
					applyHelperValueSpecBindings(
						valueSpec,
						bindings,
						importLocals,
						exportedTypes,
						helperTargets,
					)
				}
			}
		case *ast.AssignStmt:
			for _, rhs := range stmt.Rhs {
				collectCallSiteParamSpecializationsInExpr(
					rhs,
					bindings,
					currentPkg,
					info,
					importAliases,
					importLocals,
					exportedTypes,
					helperTargets,
					paramMembersByFunc,
					specializations,
				)
			}
			applyHelperAssignBindings(stmt, bindings, importLocals, exportedTypes, helperTargets)
		case *ast.ExprStmt:
			collectCallSiteParamSpecializationsInExpr(
				stmt.X,
				bindings,
				currentPkg,
				info,
				importAliases,
				importLocals,
				exportedTypes,
				helperTargets,
				paramMembersByFunc,
				specializations,
			)
		case *ast.ReturnStmt:
			for _, result := range stmt.Results {
				collectCallSiteParamSpecializationsInExpr(
					result,
					bindings,
					currentPkg,
					info,
					importAliases,
					importLocals,
					exportedTypes,
					helperTargets,
					paramMembersByFunc,
					specializations,
				)
			}
		case *ast.IfStmt:
			baseBindings := cloneBindings(bindings)
			if stmt.Init != nil {
				collectCallSiteParamSpecializations(
					[]ast.Stmt{stmt.Init},
					baseBindings,
					currentPkg,
					info,
					importAliases,
					importLocals,
					exportedTypes,
					helperTargets,
					paramMembersByFunc,
					specializations,
				)
			}
			if stmt.Cond != nil {
				collectCallSiteParamSpecializationsInExpr(
					stmt.Cond,
					baseBindings,
					currentPkg,
					info,
					importAliases,
					importLocals,
					exportedTypes,
					helperTargets,
					paramMembersByFunc,
					specializations,
				)
			}
			thenBindings := cloneBindings(baseBindings)
			collectCallSiteParamSpecializations(
				stmt.Body.List,
				thenBindings,
				currentPkg,
				info,
				importAliases,
				importLocals,
				exportedTypes,
				helperTargets,
				paramMembersByFunc,
				specializations,
			)
			if stmt.Else != nil {
				elseBindings := cloneBindings(baseBindings)
				switch elseStmt := stmt.Else.(type) {
				case *ast.BlockStmt:
					collectCallSiteParamSpecializations(
						elseStmt.List,
						elseBindings,
						currentPkg,
						info,
						importAliases,
						importLocals,
						exportedTypes,
						helperTargets,
						paramMembersByFunc,
						specializations,
					)
				case *ast.IfStmt:
					collectCallSiteParamSpecializations(
						[]ast.Stmt{elseStmt},
						elseBindings,
						currentPkg,
						info,
						importAliases,
						importLocals,
						exportedTypes,
						helperTargets,
						paramMembersByFunc,
						specializations,
					)
				}
				mergeConsistentBindings(bindings, thenBindings, elseBindings)
			}
		case *ast.SwitchStmt:
			baseBindings := cloneBindings(bindings)
			if stmt.Init != nil {
				collectCallSiteParamSpecializations(
					[]ast.Stmt{stmt.Init},
					baseBindings,
					currentPkg,
					info,
					importAliases,
					importLocals,
					exportedTypes,
					helperTargets,
					paramMembersByFunc,
					specializations,
				)
			}
			if stmt.Tag != nil {
				collectCallSiteParamSpecializationsInExpr(
					stmt.Tag,
					baseBindings,
					currentPkg,
					info,
					importAliases,
					importLocals,
					exportedTypes,
					helperTargets,
					paramMembersByFunc,
					specializations,
				)
			}
			var caseBindings []map[string]string
			hasDefault := false
			for _, clauseNode := range stmt.Body.List {
				clause, ok := clauseNode.(*ast.CaseClause)
				if !ok {
					continue
				}
				if len(clause.List) == 0 {
					hasDefault = true
				}
				branchBindings := cloneBindings(baseBindings)
				for _, expr := range clause.List {
					collectCallSiteParamSpecializationsInExpr(
						expr,
						branchBindings,
						currentPkg,
						info,
						importAliases,
						importLocals,
						exportedTypes,
						helperTargets,
						paramMembersByFunc,
						specializations,
					)
				}
				collectCallSiteParamSpecializations(
					clause.Body,
					branchBindings,
					currentPkg,
					info,
					importAliases,
					importLocals,
					exportedTypes,
					helperTargets,
					paramMembersByFunc,
					specializations,
				)
				caseBindings = append(caseBindings, branchBindings)
			}
			if hasDefault {
				mergeConsistentBindingsAcross(bindings, caseBindings)
			}
		}
	}
}

func collectCallSiteParamSpecializationsInExpr(
	expr ast.Expr,
	bindings map[string]string,
	currentPkg *types.Package,
	info *types.Info,
	importAliases map[string]string,
	importLocals map[string]struct{},
	exportedTypes map[string]struct{},
	helperTargets helperTargets,
	paramMembersByFunc map[string]map[int]map[string]struct{},
	specializations map[paramMemberKey]*paramMemberSpecialization,
) {
	switch expr := expr.(type) {
	case *ast.CallExpr:
		recordCallSiteParamSpecialization(
			expr,
			bindings,
			currentPkg,
			info,
			importAliases,
			importLocals,
			exportedTypes,
			helperTargets,
			paramMembersByFunc,
			specializations,
		)
		collectCallSiteParamSpecializationsInExpr(
			expr.Fun,
			bindings,
			currentPkg,
			info,
			importAliases,
			importLocals,
			exportedTypes,
			helperTargets,
			paramMembersByFunc,
			specializations,
		)
		for _, arg := range expr.Args {
			collectCallSiteParamSpecializationsInExpr(
				arg,
				bindings,
				currentPkg,
				info,
				importAliases,
				importLocals,
				exportedTypes,
				helperTargets,
				paramMembersByFunc,
				specializations,
			)
		}
	case *ast.SelectorExpr:
		collectCallSiteParamSpecializationsInExpr(
			expr.X,
			bindings,
			currentPkg,
			info,
			importAliases,
			importLocals,
			exportedTypes,
			helperTargets,
			paramMembersByFunc,
			specializations,
		)
	case *ast.CompositeLit:
		if expr.Type != nil {
			collectCallSiteParamSpecializationsInExpr(
				expr.Type,
				bindings,
				currentPkg,
				info,
				importAliases,
				importLocals,
				exportedTypes,
				helperTargets,
				paramMembersByFunc,
				specializations,
			)
		}
		for _, elt := range expr.Elts {
			if child, ok := elt.(ast.Expr); ok {
				collectCallSiteParamSpecializationsInExpr(
					child,
					bindings,
					currentPkg,
					info,
					importAliases,
					importLocals,
					exportedTypes,
					helperTargets,
					paramMembersByFunc,
					specializations,
				)
			}
		}
	case *ast.UnaryExpr:
		collectCallSiteParamSpecializationsInExpr(
			expr.X,
			bindings,
			currentPkg,
			info,
			importAliases,
			importLocals,
			exportedTypes,
			helperTargets,
			paramMembersByFunc,
			specializations,
		)
	case *ast.BinaryExpr:
		collectCallSiteParamSpecializationsInExpr(
			expr.X,
			bindings,
			currentPkg,
			info,
			importAliases,
			importLocals,
			exportedTypes,
			helperTargets,
			paramMembersByFunc,
			specializations,
		)
		collectCallSiteParamSpecializationsInExpr(
			expr.Y,
			bindings,
			currentPkg,
			info,
			importAliases,
			importLocals,
			exportedTypes,
			helperTargets,
			paramMembersByFunc,
			specializations,
		)
	case *ast.ParenExpr:
		collectCallSiteParamSpecializationsInExpr(
			expr.X,
			bindings,
			currentPkg,
			info,
			importAliases,
			importLocals,
			exportedTypes,
			helperTargets,
			paramMembersByFunc,
			specializations,
		)
	}
}

func recordCallSiteParamSpecialization(
	call *ast.CallExpr,
	bindings map[string]string,
	currentPkg *types.Package,
	info *types.Info,
	importAliases map[string]string,
	importLocals map[string]struct{},
	exportedTypes map[string]struct{},
	helperTargets helperTargets,
	paramMembersByFunc map[string]map[int]map[string]struct{},
	specializations map[paramMemberKey]*paramMemberSpecialization,
) {
	ident, ok := unwrapGenericCallTargetExpr(call.Fun).(*ast.Ident)
	if !ok {
		return
	}
	funcObj, ok := info.Uses[ident].(*types.Func)
	if !ok || funcObj.Pkg() != currentPkg || ast.IsExported(funcObj.Name()) {
		return
	}
	paramMembers := paramMembersByFunc[funcObj.Name()]
	if len(paramMembers) == 0 {
		return
	}
	for paramIndex, members := range paramMembers {
		for member := range members {
			key := paramMemberKey{funcName: funcObj.Name(), paramIndex: paramIndex, member: member}
			spec := specializations[key]
			if spec == nil {
				spec = &paramMemberSpecialization{targets: map[string]struct{}{}}
				specializations[key] = spec
			}
			spec.callCount++
			if paramIndex >= len(call.Args) {
				spec.unresolved = true
				continue
			}
			target := resolveExprTarget(
				call.Args[paramIndex],
				bindings,
				importLocals,
				exportedTypes,
				helperTargets,
			)
			if target == "" {
				if typeAndValue, ok := info.Types[call.Args[paramIndex]]; ok {
					target = namedTypeTargetFromTypes(typeAndValue.Type, currentPkg, importAliases)
				}
			}
			if target == "" {
				spec.unresolved = true
				continue
			}
			spec.targets[target] = struct{}{}
		}
	}
}

func collectImplementedInterfaces(currentPath string, source []byte) []typeHeritage {
	fset := token.NewFileSet()
	files, primary := parseFilesForTypeChecking(fset, currentPath, source)
	if len(files) == 0 || primary == nil {
		return nil
	}
	importAliases := collectImportAliases(primary)

	importer := newSourceImporter(fset, currentPath)
	conf := types.Config{
		Importer: importer,
		Error:    func(error) {},
	}
	pkg, _ := conf.Check(primary.Name.Name, fset, files, nil)
	if pkg == nil {
		return nil
	}

	currentInterfaces := map[string]*types.Interface{}
	scopeNames := pkg.Scope().Names()
	for _, name := range scopeNames {
		if !ast.IsExported(name) {
			continue
		}
		obj := pkg.Scope().Lookup(name)
		if obj == nil {
			continue
		}
		iface := interfaceFromType(obj.Type())
		if iface == nil {
			continue
		}
		currentInterfaces[name] = iface
	}

	importedInterfaces := map[string]*types.Interface{}
	for _, spec := range primary.Imports {
		importPath := strings.Trim(spec.Path.Value, "\"")
		localName, ok := importAliases[importPath]
		if !ok {
			continue
		}
		importedPkg, err := importer.ImportFrom(importPath, filepath.Dir(currentPath), 0)
		if err != nil || importedPkg == nil {
			continue
		}
		for _, name := range importedPkg.Scope().Names() {
			if !ast.IsExported(name) {
				continue
			}
			obj := importedPkg.Scope().Lookup(name)
			if obj == nil {
				continue
			}
			iface := interfaceFromType(obj.Type())
			if iface == nil {
				continue
			}
			importedInterfaces[localName+"."+name] = iface
		}
	}

	var heritage []typeHeritage
	for _, name := range pkg.Scope().Names() {
		if !ast.IsExported(name) {
			continue
		}
		obj := pkg.Scope().Lookup(name)
		if obj == nil {
			continue
		}
		named, ok := obj.Type().(*types.Named)
		if !ok || interfaceFromType(named) != nil {
			continue
		}

		implements := []string{}
		for interfaceName, iface := range currentInterfaces {
			if interfaceName == name {
				continue
			}
			if namedImplementsInterface(named, iface) {
				implements = append(implements, interfaceName)
			}
		}
		for interfaceName, iface := range importedInterfaces {
			if namedImplementsInterface(named, iface) {
				implements = append(implements, interfaceName)
			}
		}
		if len(implements) == 0 {
			continue
		}
		sort.Strings(implements)
		heritage = append(heritage, typeHeritage{
			ExportName: name,
			Implements: dedupeStrings(implements),
		})
	}

	sort.Slice(heritage, func(i, j int) bool {
		return heritage[i].ExportName < heritage[j].ExportName
	})
	return heritage
}

type sourceImporter struct {
	fset       *token.FileSet
	currentDir string
	cache      map[string]*types.Package
	importing  map[string]bool
}

type goListPackage struct {
	Dir        string   `json:"Dir"`
	ImportPath string   `json:"ImportPath"`
	Name       string   `json:"Name"`
	GoFiles    []string `json:"GoFiles"`
}

func newSourceImporter(fset *token.FileSet, currentPath string) *sourceImporter {
	return &sourceImporter{
		fset:       fset,
		currentDir: filepath.Dir(currentPath),
		cache:      map[string]*types.Package{},
		importing:  map[string]bool{},
	}
}

func (s *sourceImporter) Import(path string) (*types.Package, error) {
	return s.ImportFrom(path, s.currentDir, 0)
}

func (s *sourceImporter) ImportFrom(path, dir string, _ types.ImportMode) (*types.Package, error) {
	if pkg, ok := s.cache[path]; ok {
		return pkg, nil
	}
	if s.importing[path] {
		return nil, nil
	}

	pkgInfo, err := s.goListPackage(path, dir)
	if err != nil {
		return nil, err
	}

	s.importing[path] = true
	defer delete(s.importing, path)

	files := make([]*ast.File, 0, len(pkgInfo.GoFiles))
	for _, name := range pkgInfo.GoFiles {
		fullPath := filepath.Join(pkgInfo.Dir, name)
		file, err := parser.ParseFile(s.fset, fullPath, nil, parser.SkipObjectResolution)
		if err != nil {
			return nil, err
		}
		files = append(files, file)
	}
	if len(files) == 0 {
		return nil, nil
	}

	info := &types.Info{
		Selections: map[*ast.SelectorExpr]*types.Selection{},
	}
	conf := types.Config{
		Importer: s,
		Error:    func(error) {},
	}
	pkg, err := conf.Check(pkgInfo.ImportPath, s.fset, files, info)
	if pkg != nil {
		s.cache[path] = pkg
	}
	return pkg, err
}

func (s *sourceImporter) goListPackage(path, dir string) (*goListPackage, error) {
	cmd := exec.Command("go", "list", "-json", path)
	if dir != "" && dir != "." {
		cmd.Dir = dir
	}
	output, err := cmd.Output()
	if err != nil {
		return nil, err
	}

	var pkg goListPackage
	if err := json.Unmarshal(output, &pkg); err != nil {
		return nil, err
	}
	return &pkg, nil
}

func parseFilesForTypeChecking(
	fset *token.FileSet,
	currentPath string,
	source []byte,
) ([]*ast.File, *ast.File) {
	primary, err := parser.ParseFile(fset, currentPath, source, parser.SkipObjectResolution)
	if err != nil {
		return nil, nil
	}
	files := []*ast.File{primary}
	dir := filepath.Dir(currentPath)
	entries, err := os.ReadDir(dir)
	if err != nil {
		return files, primary
	}
	for _, entry := range entries {
		if entry.IsDir() {
			continue
		}
		name := entry.Name()
		if filepath.Ext(name) != ".go" || strings.HasSuffix(name, "_test.go") {
			continue
		}
		fullPath := filepath.Join(dir, name)
		if fullPath == currentPath {
			continue
		}
		file, err := parser.ParseFile(fset, fullPath, nil, parser.SkipObjectResolution)
		if err != nil || file.Name == nil || file.Name.Name != primary.Name.Name {
			continue
		}
		files = append(files, file)
	}
	return files, primary
}

func namedTypeTargetFromTypes(
	recv types.Type,
	currentPkg *types.Package,
	importAliases map[string]string,
) string {
	for {
		switch t := recv.(type) {
		case *types.Pointer:
			recv = t.Elem()
		case *types.Named:
			obj := t.Obj()
			if obj == nil || !obj.Exported() {
				return ""
			}
			if obj.Pkg() == currentPkg {
				return obj.Name()
			}
			if obj.Pkg() != nil {
				if localName, ok := importAliases[obj.Pkg().Path()]; ok {
					return localName + "." + obj.Name()
				}
			}
			return ""
		case *types.TypeParam:
			recv = t.Constraint()
		case *types.Alias:
			obj := t.Obj()
			if obj == nil || !obj.Exported() {
				return ""
			}
			if obj.Pkg() == currentPkg {
				return obj.Name()
			}
			if obj.Pkg() != nil {
				if localName, ok := importAliases[obj.Pkg().Path()]; ok {
					return localName + "." + obj.Name()
				}
			}
			return ""
		default:
			return ""
		}
	}
}

func interfaceFromType(typ types.Type) *types.Interface {
	switch t := typ.(type) {
	case *types.Named:
		iface, ok := types.Unalias(t).Underlying().(*types.Interface)
		if ok {
			return iface.Complete()
		}
	case *types.Alias:
		iface, ok := types.Unalias(t).Underlying().(*types.Interface)
		if ok {
			return iface.Complete()
		}
	default:
		iface, ok := types.Unalias(t).Underlying().(*types.Interface)
		if ok {
			return iface.Complete()
		}
	}
	return nil
}

func namedImplementsInterface(named *types.Named, iface *types.Interface) bool {
	if named == nil || iface == nil {
		return false
	}
	if types.Implements(named, iface) {
		return true
	}
	return types.Implements(types.NewPointer(named), iface)
}

func dedupeStrings(values []string) []string {
	if len(values) < 2 {
		return values
	}
	out := values[:0]
	for _, value := range values {
		if len(out) > 0 && out[len(out)-1] == value {
			continue
		}
		out = append(out, value)
	}
	return out
}

func dedupeMemberAccesses(accesses []memberAccess) []memberAccess {
	if len(accesses) < 2 {
		return accesses
	}
	sort.Slice(accesses, func(i, j int) bool {
		if accesses[i].Object == accesses[j].Object {
			return accesses[i].Member < accesses[j].Member
		}
		return accesses[i].Object < accesses[j].Object
	})
	out := accesses[:0]
	for _, access := range accesses {
		if len(out) > 0 && out[len(out)-1] == access {
			continue
		}
		out = append(out, access)
	}
	return out
}

func processBlock(
	stmts []ast.Stmt,
	bindings map[string]string,
	importLocals map[string]struct{},
	exportedTypes map[string]struct{},
	helperTargets helperTargets,
	seen map[string]struct{},
	accesses *[]memberAccess,
) {
	for _, stmt := range stmts {
		switch stmt := stmt.(type) {
		case *ast.DeclStmt:
			genDecl, ok := stmt.Decl.(*ast.GenDecl)
			if !ok || genDecl.Tok != token.VAR {
				continue
			}
			for _, spec := range genDecl.Specs {
				valueSpec, ok := spec.(*ast.ValueSpec)
				if !ok {
					continue
				}
				processValueSpec(
					valueSpec,
					bindings,
					importLocals,
					exportedTypes,
					helperTargets,
					seen,
					accesses,
				)
			}
		case *ast.AssignStmt:
			processAssign(
				stmt,
				bindings,
				importLocals,
				exportedTypes,
				helperTargets,
				seen,
				accesses,
			)
		case *ast.ExprStmt:
			walkExpr(stmt.X, bindings, importLocals, exportedTypes, helperTargets, seen, accesses)
		case *ast.ReturnStmt:
			for _, expr := range stmt.Results {
				walkExpr(expr, bindings, importLocals, exportedTypes, helperTargets, seen, accesses)
			}
		case *ast.BlockStmt:
			processBlock(
				stmt.List,
				cloneBindings(bindings),
				importLocals,
				exportedTypes,
				helperTargets,
				seen,
				accesses,
			)
		case *ast.IfStmt:
			processIfStmt(stmt, bindings, importLocals, exportedTypes, helperTargets, seen, accesses)
		case *ast.SwitchStmt:
			processSwitchStmt(stmt, bindings, importLocals, exportedTypes, helperTargets, seen, accesses)
		default:
			ast.Inspect(stmt, func(node ast.Node) bool {
				expr, ok := node.(ast.Expr)
				if ok {
					walkExpr(expr, bindings, importLocals, exportedTypes, helperTargets, seen, accesses)
					return false
				}
				return true
			})
		}
	}
}

func processValueSpec(
	spec *ast.ValueSpec,
	bindings map[string]string,
	importLocals map[string]struct{},
	exportedTypes map[string]struct{},
	helperTargets helperTargets,
	seen map[string]struct{},
	accesses *[]memberAccess,
) {
	var annotatedTarget string
	if spec.Type != nil {
		annotatedTarget = resolveTypeTarget(spec.Type, importLocals, exportedTypes)
	}
	for i, name := range spec.Names {
		if name == nil {
			continue
		}
		var rhsTarget string
		if i < len(spec.Values) {
			walkExpr(spec.Values[i], bindings, importLocals, exportedTypes, helperTargets, seen, accesses)
			rhsTarget = resolveExprTarget(
				spec.Values[i],
				bindings,
				importLocals,
				exportedTypes,
				helperTargets,
			)
		}
		if rhsTarget != "" {
			bindings[name.Name] = rhsTarget
		} else if annotatedTarget != "" {
			bindings[name.Name] = annotatedTarget
		}
	}
}

func processAssign(
	stmt *ast.AssignStmt,
	bindings map[string]string,
	importLocals map[string]struct{},
	exportedTypes map[string]struct{},
	helperTargets helperTargets,
	seen map[string]struct{},
	accesses *[]memberAccess,
) {
	for idx, rhs := range stmt.Rhs {
		walkExpr(rhs, bindings, importLocals, exportedTypes, helperTargets, seen, accesses)
		if idx >= len(stmt.Lhs) {
			continue
		}
		ident, ok := stmt.Lhs[idx].(*ast.Ident)
		if !ok {
			continue
		}
		target := resolveExprTarget(rhs, bindings, importLocals, exportedTypes, helperTargets)
		if target != "" {
			bindings[ident.Name] = target
		}
	}
}

func processIfStmt(
	stmt *ast.IfStmt,
	bindings map[string]string,
	importLocals map[string]struct{},
	exportedTypes map[string]struct{},
	helperTargets helperTargets,
	seen map[string]struct{},
	accesses *[]memberAccess,
) {
	baseBindings := cloneBindings(bindings)
	if stmt.Init != nil {
		processBlock(
			[]ast.Stmt{stmt.Init},
			baseBindings,
			importLocals,
			exportedTypes,
			helperTargets,
			seen,
			accesses,
		)
	}
	if stmt.Cond != nil {
		walkExpr(stmt.Cond, baseBindings, importLocals, exportedTypes, helperTargets, seen, accesses)
	}

	thenBindings := cloneBindings(baseBindings)
	processBlock(
		stmt.Body.List,
		thenBindings,
		importLocals,
		exportedTypes,
		helperTargets,
		seen,
		accesses,
	)

	if stmt.Else == nil {
		return
	}

	elseBindings := cloneBindings(baseBindings)
	switch elseStmt := stmt.Else.(type) {
	case *ast.BlockStmt:
		processBlock(
			elseStmt.List,
			elseBindings,
			importLocals,
			exportedTypes,
			helperTargets,
			seen,
			accesses,
		)
	case *ast.IfStmt:
		processIfStmt(
			elseStmt,
			elseBindings,
			importLocals,
			exportedTypes,
			helperTargets,
			seen,
			accesses,
		)
	default:
		return
	}

	mergeConsistentBindings(bindings, thenBindings, elseBindings)
}

func processSwitchStmt(
	stmt *ast.SwitchStmt,
	bindings map[string]string,
	importLocals map[string]struct{},
	exportedTypes map[string]struct{},
	helperTargets helperTargets,
	seen map[string]struct{},
	accesses *[]memberAccess,
) {
	baseBindings := cloneBindings(bindings)
	if stmt.Init != nil {
		processBlock(
			[]ast.Stmt{stmt.Init},
			baseBindings,
			importLocals,
			exportedTypes,
			helperTargets,
			seen,
			accesses,
		)
	}
	if stmt.Tag != nil {
		walkExpr(stmt.Tag, baseBindings, importLocals, exportedTypes, helperTargets, seen, accesses)
	}

	if len(stmt.Body.List) == 0 {
		return
	}

	var caseBindings []map[string]string
	hasDefault := false
	for _, clauseNode := range stmt.Body.List {
		clause, ok := clauseNode.(*ast.CaseClause)
		if !ok {
			return
		}
		if len(clause.List) == 0 {
			hasDefault = true
		}
		branchBindings := cloneBindings(baseBindings)
		for _, expr := range clause.List {
			walkExpr(expr, branchBindings, importLocals, exportedTypes, helperTargets, seen, accesses)
		}
		processBlock(
			clause.Body,
			branchBindings,
			importLocals,
			exportedTypes,
			helperTargets,
			seen,
			accesses,
		)
		caseBindings = append(caseBindings, branchBindings)
	}

	if !hasDefault {
		return
	}
	mergeConsistentBindingsAcross(bindings, caseBindings)
}

func mergeConsistentBindings(
	bindings map[string]string,
	thenBindings map[string]string,
	elseBindings map[string]string,
) {
	for name := range bindings {
		thenTarget, thenOk := thenBindings[name]
		elseTarget, elseOk := elseBindings[name]
		if !thenOk || !elseOk || thenTarget == "" || elseTarget == "" || thenTarget != elseTarget {
			continue
		}
		bindings[name] = thenTarget
	}
}

func mergeConsistentBindingsAcross(bindings map[string]string, branches []map[string]string) {
	if len(branches) == 0 {
		return
	}
	for name := range bindings {
		target, ok := branches[0][name]
		if !ok || target == "" {
			continue
		}
		consistent := true
		for _, branch := range branches[1:] {
			branchTarget, ok := branch[name]
			if !ok || branchTarget == "" || branchTarget != target {
				consistent = false
				break
			}
		}
		if consistent {
			bindings[name] = target
		}
	}
}

func walkExpr(
	expr ast.Expr,
	bindings map[string]string,
	importLocals map[string]struct{},
	exportedTypes map[string]struct{},
	helperTargets helperTargets,
	seen map[string]struct{},
	accesses *[]memberAccess,
) {
	switch expr := expr.(type) {
	case *ast.CallExpr:
		walkExpr(expr.Fun, bindings, importLocals, exportedTypes, helperTargets, seen, accesses)
		for _, arg := range expr.Args {
			walkExpr(arg, bindings, importLocals, exportedTypes, helperTargets, seen, accesses)
		}
	case *ast.SelectorExpr:
		if ast.IsExported(expr.Sel.Name) {
			if target := resolveExprTarget(expr.X, bindings, importLocals, exportedTypes, helperTargets); target != "" {
				key := target + "." + expr.Sel.Name
				if _, duplicate := seen[key]; !duplicate {
					*accesses = append(*accesses, memberAccess{Object: target, Member: expr.Sel.Name})
					seen[key] = struct{}{}
				}
			}
		}
		walkExpr(expr.X, bindings, importLocals, exportedTypes, helperTargets, seen, accesses)
	case *ast.CompositeLit:
		if expr.Type != nil {
			walkExpr(expr.Type, bindings, importLocals, exportedTypes, helperTargets, seen, accesses)
		}
		for _, elt := range expr.Elts {
			if node, ok := elt.(ast.Expr); ok {
				walkExpr(node, bindings, importLocals, exportedTypes, helperTargets, seen, accesses)
			}
		}
	case *ast.UnaryExpr:
		walkExpr(expr.X, bindings, importLocals, exportedTypes, helperTargets, seen, accesses)
	case *ast.BinaryExpr:
		walkExpr(expr.X, bindings, importLocals, exportedTypes, helperTargets, seen, accesses)
		walkExpr(expr.Y, bindings, importLocals, exportedTypes, helperTargets, seen, accesses)
	case *ast.ParenExpr:
		walkExpr(expr.X, bindings, importLocals, exportedTypes, helperTargets, seen, accesses)
	case *ast.IndexExpr:
		walkExpr(expr.X, bindings, importLocals, exportedTypes, helperTargets, seen, accesses)
		walkExpr(expr.Index, bindings, importLocals, exportedTypes, helperTargets, seen, accesses)
	case *ast.IndexListExpr:
		walkExpr(expr.X, bindings, importLocals, exportedTypes, helperTargets, seen, accesses)
		for _, index := range expr.Indices {
			walkExpr(index, bindings, importLocals, exportedTypes, helperTargets, seen, accesses)
		}
	case *ast.SliceExpr:
		walkExpr(expr.X, bindings, importLocals, exportedTypes, helperTargets, seen, accesses)
	case *ast.KeyValueExpr:
		walkExpr(expr.Key, bindings, importLocals, exportedTypes, helperTargets, seen, accesses)
		walkExpr(expr.Value, bindings, importLocals, exportedTypes, helperTargets, seen, accesses)
	}
}

func resolveExprTarget(
	expr ast.Expr,
	bindings map[string]string,
	importLocals map[string]struct{},
	exportedTypes map[string]struct{},
	helperTargets helperTargets,
) string {
	switch expr := expr.(type) {
	case *ast.CompositeLit:
		return resolveTypeTarget(expr.Type, importLocals, exportedTypes)
	case *ast.UnaryExpr:
		if expr.Op == token.AND {
			return resolveExprTarget(expr.X, bindings, importLocals, exportedTypes, helperTargets)
		}
	case *ast.Ident:
		return bindings[expr.Name]
	case *ast.CallExpr:
		return resolveCallTarget(expr, bindings, importLocals, exportedTypes, helperTargets)
	case *ast.ParenExpr:
		return resolveExprTarget(expr.X, bindings, importLocals, exportedTypes, helperTargets)
	}
	return ""
}

func resolveTypeTarget(
	expr ast.Expr,
	importLocals map[string]struct{},
	exportedTypes map[string]struct{},
) string {
	switch expr := expr.(type) {
	case *ast.Ident:
		if _, ok := exportedTypes[expr.Name]; ok {
			return expr.Name
		}
	case *ast.SelectorExpr:
		if ident, ok := expr.X.(*ast.Ident); ok {
			if _, imported := importLocals[ident.Name]; imported && expr.Sel.IsExported() {
				return ident.Name + "." + expr.Sel.Name
			}
		}
	case *ast.IndexExpr:
		return resolveTypeTarget(expr.X, importLocals, exportedTypes)
	case *ast.IndexListExpr:
		return resolveTypeTarget(expr.X, importLocals, exportedTypes)
	}
	return ""
}

func resolveCallTarget(
	call *ast.CallExpr,
	bindings map[string]string,
	importLocals map[string]struct{},
	exportedTypes map[string]struct{},
	helperTargets helperTargets,
) string {
	fun := unwrapGenericCallTargetExpr(call.Fun)
	if ident, ok := fun.(*ast.Ident); ok {
		if ident.Name == "new" && len(call.Args) == 1 {
			return resolveTypeTarget(call.Args[0], importLocals, exportedTypes)
		}
		if target, ok := helperTargets.fixed[ident.Name]; ok {
			return target
		}
		if argIndex, ok := helperTargets.passthroughArgIndex[ident.Name]; ok && argIndex < len(call.Args) {
			return resolveExprTarget(call.Args[argIndex], bindings, importLocals, exportedTypes, helperTargets)
		}
		if typeName, ok := strings.CutPrefix(ident.Name, "New"); ok {
			if _, exported := exportedTypes[typeName]; exported && typeName != "" {
				return typeName
			}
		}
	}
	if selector, ok := fun.(*ast.SelectorExpr); ok {
		if ident, ok := selector.X.(*ast.Ident); ok {
			if _, imported := importLocals[ident.Name]; imported {
				qualifiedName := ident.Name + "." + selector.Sel.Name
				if target, ok := helperTargets.qualifiedFixed[qualifiedName]; ok {
					return target
				}
				if argIndex, ok := helperTargets.qualifiedPassthroughArgIndex[qualifiedName]; ok && argIndex < len(call.Args) {
					return resolveExprTarget(call.Args[argIndex], bindings, importLocals, exportedTypes, helperTargets)
				}
				if typeName, ok := strings.CutPrefix(selector.Sel.Name, "New"); ok && typeName != "" && ast.IsExported(typeName) {
					return ident.Name + "." + typeName
				}
			}
		}
	}
	return ""
}

func unwrapGenericCallTargetExpr(expr ast.Expr) ast.Expr {
	for {
		switch expr := expr.(type) {
		case *ast.IndexExpr:
			expr = expr
			return unwrapGenericCallTargetExpr(expr.X)
		case *ast.IndexListExpr:
			expr = expr
			return unwrapGenericCallTargetExpr(expr.X)
		case *ast.ParenExpr:
			expr = expr
			return unwrapGenericCallTargetExpr(expr.X)
		default:
			return expr
		}
	}
}

type helperTargets struct {
	fixed                        map[string]string
	passthroughArgIndex          map[string]int
	qualifiedFixed               map[string]string
	qualifiedPassthroughArgIndex map[string]int
}

func collectHelperTargets(
	currentPath string,
	file *ast.File,
	importLocals map[string]struct{},
	exportedTypes map[string]struct{},
) helperTargets {
	targets := helperTargets{
		fixed:                        map[string]string{},
		passthroughArgIndex:          map[string]int{},
		qualifiedFixed:               map[string]string{},
		qualifiedPassthroughArgIndex: map[string]int{},
	}

	var funcs []*ast.FuncDecl
	for _, decl := range file.Decls {
		funcDecl, ok := decl.(*ast.FuncDecl)
		if !ok || funcDecl.Recv != nil || funcDecl.Body == nil || funcDecl.Name == nil {
			continue
		}
		funcs = append(funcs, funcDecl)
	}

	collectImportedHelperTargets(currentPath, file, &targets)

	for {
		changed := false
		for _, funcDecl := range funcs {
			name := funcDecl.Name.Name
			signatureTarget := resolveResultTarget(funcDecl.Type.Results, importLocals, exportedTypes)
			paramNames := helperParamNames(funcDecl.Type.Params)
			bodyTarget, resolvable := scanHelperBodyTarget(
				funcDecl.Body,
				paramNames,
				importLocals,
				exportedTypes,
				targets,
			)
			if bodyTarget == "" &&
				resolvable {
				if argIndex, ok := helperReturnsParamThroughLocalBindings(
					funcDecl.Body,
					paramNames,
					importLocals,
					exportedTypes,
					targets,
				); ok {
					if _, exists := targets.fixed[name]; !exists {
						if current, exists := targets.passthroughArgIndex[name]; !exists || current != argIndex {
							targets.passthroughArgIndex[name] = argIndex
							changed = true
						}
					}
					continue
				}
			}

			resolvedTarget := bodyTarget
			if resolvedTarget == "" {
				resolvedTarget = signatureTarget
			}
			if resolvedTarget != "" && targets.fixed[name] != resolvedTarget {
				targets.fixed[name] = resolvedTarget
				delete(targets.passthroughArgIndex, name)
				changed = true
			}
		}
		if !changed {
			break
		}
	}
	return targets
}

func collectImportedHelperTargets(currentPath string, file *ast.File, targets *helperTargets) {
	if currentPath == "" || file == nil {
		return
	}
	fset := token.NewFileSet()
	importer := newSourceImporter(fset, currentPath)
	currentDir := filepath.Dir(currentPath)
	for _, spec := range file.Imports {
		importPath := strings.Trim(spec.Path.Value, "\"")
		localName := path.Base(importPath)
		if spec.Name != nil {
			if spec.Name.Name == "_" || spec.Name.Name == "." {
				continue
			}
			localName = spec.Name.Name
		}

		pkgInfo, err := importer.goListPackage(importPath, currentDir)
		if err != nil || pkgInfo == nil {
			continue
		}

		pkgFiles := make([]*ast.File, 0, len(pkgInfo.GoFiles))
		exportedTypes := map[string]struct{}{}
		for _, name := range pkgInfo.GoFiles {
			fullPath := filepath.Join(pkgInfo.Dir, name)
			pkgFile, err := parser.ParseFile(fset, fullPath, nil, parser.SkipObjectResolution)
			if err != nil {
				pkgFiles = nil
				break
			}
			pkgFiles = append(pkgFiles, pkgFile)
			for exportedType := range collectExportedTypes(pkgFile) {
				exportedTypes[exportedType] = struct{}{}
			}
		}
		if len(pkgFiles) == 0 {
			continue
		}

		for _, pkgFile := range pkgFiles {
			fileTargets := collectHelperTargets("", pkgFile, collectImportLocals(pkgFile), exportedTypes)
			for name, target := range fileTargets.fixed {
				targets.qualifiedFixed[localName+"."+name] = localName + "." + target
			}
			for name, argIndex := range fileTargets.passthroughArgIndex {
				targets.qualifiedPassthroughArgIndex[localName+"."+name] = argIndex
			}
		}
	}
}

func collectExportedTypes(file *ast.File) map[string]struct{} {
	exportedTypes := map[string]struct{}{}
	if file == nil {
		return exportedTypes
	}
	for _, decl := range file.Decls {
		genDecl, ok := decl.(*ast.GenDecl)
		if !ok || genDecl.Tok != token.TYPE {
			continue
		}
		for _, spec := range genDecl.Specs {
			typeSpec, ok := spec.(*ast.TypeSpec)
			if ok && typeSpec.Name.IsExported() {
				exportedTypes[typeSpec.Name.Name] = struct{}{}
			}
		}
	}
	return exportedTypes
}

func resolveResultTarget(
	results *ast.FieldList,
	importLocals map[string]struct{},
	exportedTypes map[string]struct{},
) string {
	if results == nil || len(results.List) != 1 {
		return ""
	}
	field := results.List[0]
	if len(field.Names) > 1 {
		return ""
	}
	return resolveTypeTarget(field.Type, importLocals, exportedTypes)
}

func helperParamNames(params *ast.FieldList) []string {
	if params == nil {
		return nil
	}
	var names []string
	for _, field := range params.List {
		if len(field.Names) == 0 {
			return nil
		}
		for _, name := range field.Names {
			names = append(names, name.Name)
		}
	}
	return names
}

func helperReturnsParamThroughLocalBindings(
	body *ast.BlockStmt,
	paramNames []string,
	importLocals map[string]struct{},
	exportedTypes map[string]struct{},
	helperTargets helperTargets,
) (int, bool) {
	if body == nil || len(paramNames) == 0 {
		return 0, false
	}
	bindings := map[string]string{}
	for index, paramName := range paramNames {
		bindings[paramName] = helperParamSentinel(index)
	}
	for _, stmt := range body.List {
		switch stmt := stmt.(type) {
		case *ast.DeclStmt:
			genDecl, ok := stmt.Decl.(*ast.GenDecl)
			if !ok || genDecl.Tok != token.VAR {
				return 0, false
			}
			for _, spec := range genDecl.Specs {
				valueSpec, ok := spec.(*ast.ValueSpec)
				if !ok {
					return 0, false
				}
				applyHelperValueSpecBindings(
					valueSpec,
					bindings,
					importLocals,
					exportedTypes,
					helperTargets,
				)
			}
		case *ast.AssignStmt:
			applyHelperAssignBindings(stmt, bindings, importLocals, exportedTypes, helperTargets)
		case *ast.ReturnStmt:
			if len(stmt.Results) != 1 {
				return 0, false
			}
			target := resolveExprTarget(
				stmt.Results[0],
				bindings,
				importLocals,
				exportedTypes,
				helperTargets,
			)
			return parseHelperParamSentinel(target)
		case *ast.EmptyStmt:
			continue
		default:
			return 0, false
		}
	}
	return 0, false
}

func scanHelperBodyTarget(
	body *ast.BlockStmt,
	paramNames []string,
	importLocals map[string]struct{},
	exportedTypes map[string]struct{},
	helperTargets helperTargets,
) (string, bool) {
	if body == nil {
		return "", false
	}
	if target, resolvable, supported := scanLinearHelperBodyTarget(
		body.List,
		paramNames,
		importLocals,
		exportedTypes,
		helperTargets,
	); supported {
		return target, resolvable
	}

	type state struct {
		target          string
		sawReturn       bool
		sawResolvable   bool
		sawPassthrough  bool
		sawUnresolvable bool
		conflict        bool
	}
	result := state{}
	paramBindings := map[string]string{}
	for index, paramName := range paramNames {
		paramBindings[paramName] = helperParamSentinel(index)
	}

	ast.Inspect(body, func(node ast.Node) bool {
		if result.conflict {
			return false
		}
		if _, ok := node.(*ast.FuncLit); ok {
			return false
		}
		ret, ok := node.(*ast.ReturnStmt)
		if !ok {
			return true
		}
		result.sawReturn = true
		if len(ret.Results) != 1 {
			result.sawUnresolvable = true
			return true
		}
		target := resolveExprTarget(
			ret.Results[0],
			paramBindings,
			importLocals,
			exportedTypes,
			helperTargets,
		)
		if target == "" {
			result.sawUnresolvable = true
			return true
		}
		if _, ok := parseHelperParamSentinel(target); ok {
			result.sawPassthrough = true
			return true
		}
		result.sawResolvable = true
		if result.target == "" {
			result.target = target
			return true
		}
		if result.target != target {
			result.conflict = true
			result.target = ""
			return false
		}
		return true
	})

	if result.conflict || !result.sawReturn {
		return "", false
	}
	if result.target != "" && result.sawPassthrough {
		return "", true
	}
	if result.target == "" && result.sawUnresolvable {
		return "", false
	}
	return result.target, true
}

const helperPassthroughSentinelPrefix = "__fallow_helper_passthrough__:"

func helperParamSentinel(index int) string {
	return helperPassthroughSentinelPrefix + strconv.Itoa(index)
}

func parseHelperParamSentinel(value string) (int, bool) {
	indexValue, ok := strings.CutPrefix(value, helperPassthroughSentinelPrefix)
	if !ok {
		return 0, false
	}
	index, err := strconv.Atoi(indexValue)
	if err != nil {
		return 0, false
	}
	return index, true
}

func scanLinearHelperBodyTarget(
	stmts []ast.Stmt,
	paramNames []string,
	importLocals map[string]struct{},
	exportedTypes map[string]struct{},
	helperTargets helperTargets,
) (string, bool, bool) {
	bindings := map[string]string{}
	for index, paramName := range paramNames {
		bindings[paramName] = helperParamSentinel(index)
	}

	type state struct {
		target          string
		sawReturn       bool
		sawPassthrough  bool
		sawUnresolvable bool
		conflict        bool
	}
	result := state{}

	for _, stmt := range stmts {
		switch stmt := stmt.(type) {
		case *ast.DeclStmt:
			genDecl, ok := stmt.Decl.(*ast.GenDecl)
			if !ok || genDecl.Tok != token.VAR {
				continue
			}
			for _, spec := range genDecl.Specs {
				valueSpec, ok := spec.(*ast.ValueSpec)
				if !ok {
					continue
				}
				applyHelperValueSpecBindings(
					valueSpec,
					bindings,
					importLocals,
					exportedTypes,
					helperTargets,
				)
			}
		case *ast.AssignStmt:
			applyHelperAssignBindings(stmt, bindings, importLocals, exportedTypes, helperTargets)
		case *ast.ReturnStmt:
			result.sawReturn = true
			if len(stmt.Results) != 1 {
				result.sawUnresolvable = true
				continue
			}
			target := resolveExprTarget(
				stmt.Results[0],
				bindings,
				importLocals,
				exportedTypes,
				helperTargets,
			)
			switch target {
			case "":
				result.sawUnresolvable = true
			default:
				if _, ok := parseHelperParamSentinel(target); ok {
					result.sawPassthrough = true
					break
				}
				if result.target == "" {
					result.target = target
				} else if result.target != target {
					result.conflict = true
				}
			}
		case *ast.EmptyStmt:
			continue
		default:
			return "", false, false
		}
	}

	if result.conflict || !result.sawReturn {
		return "", false, true
	}
	if result.target != "" && result.sawPassthrough {
		return "", true, true
	}
	if result.target == "" && result.sawUnresolvable {
		return "", false, true
	}
	return result.target, true, true
}

func applyHelperValueSpecBindings(
	spec *ast.ValueSpec,
	bindings map[string]string,
	importLocals map[string]struct{},
	exportedTypes map[string]struct{},
	helperTargets helperTargets,
) {
	var annotatedTarget string
	if spec.Type != nil {
		annotatedTarget = resolveTypeTarget(spec.Type, importLocals, exportedTypes)
	}
	for i, name := range spec.Names {
		if name == nil {
			continue
		}
		var rhsTarget string
		if i < len(spec.Values) {
			rhsTarget = resolveExprTarget(
				spec.Values[i],
				bindings,
				importLocals,
				exportedTypes,
				helperTargets,
			)
		}
		if rhsTarget != "" {
			bindings[name.Name] = rhsTarget
		} else if annotatedTarget != "" {
			bindings[name.Name] = annotatedTarget
		}
	}
}

func applyHelperAssignBindings(
	stmt *ast.AssignStmt,
	bindings map[string]string,
	importLocals map[string]struct{},
	exportedTypes map[string]struct{},
	helperTargets helperTargets,
) {
	for idx, rhs := range stmt.Rhs {
		if idx >= len(stmt.Lhs) {
			continue
		}
		ident, ok := stmt.Lhs[idx].(*ast.Ident)
		if !ok {
			continue
		}
		target := resolveExprTarget(rhs, bindings, importLocals, exportedTypes, helperTargets)
		if target != "" {
			bindings[ident.Name] = target
		}
	}
}

func cloneBindings(bindings map[string]string) map[string]string {
	next := make(map[string]string, len(bindings))
	for key, value := range bindings {
		next[key] = value
	}
	return next
}

func addMember(
	membersByType map[string]map[string]helperMember,
	fset *token.FileSet,
	typeName, memberName, kind string,
	startPos, endPos token.Pos,
) {
	entry := membersByType[typeName]
	if entry == nil {
		entry = map[string]helperMember{}
		membersByType[typeName] = entry
	}
	entry[memberName] = helperMember{
		Name:  memberName,
		Kind:  kind,
		Start: fset.PositionFor(startPos, false).Offset,
		End:   fset.PositionFor(endPos, false).Offset,
	}
}

func receiverTypeName(fields []*ast.Field) string {
	if len(fields) != 1 {
		return ""
	}
	return unwrapReceiverType(fields[0].Type)
}

func unwrapReceiverType(expr ast.Expr) string {
	switch expr := expr.(type) {
	case *ast.Ident:
		return expr.Name
	case *ast.StarExpr:
		return unwrapReceiverType(expr.X)
	case *ast.IndexExpr:
		return unwrapReceiverType(expr.X)
	case *ast.IndexListExpr:
		return unwrapReceiverType(expr.X)
	default:
		return ""
	}
}
