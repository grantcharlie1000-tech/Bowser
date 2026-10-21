#!/usr/bin/env python3
"""Portable source contracts for rooted browser MenuItems, not execution proof.

These checks inspect the actual embedded C# methods and their live call sites.
They do not compile or execute C#, run a UIA provider, send input, or establish
that a real browser menu passes the complete inventory/final Escape decision.
"""
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[1]
PROBE = ROOT / "benchmarks/scripts/probe-default-context-cancellation.ps1"
HELPER = PROBE.read_text(encoding="utf-8")
CS = HELPER.split("Add-Type -ReferencedAssemblies $uiaReferences @'\n", 1)[1].split("\n'@", 1)[0]

# Keep byte-for-byte offsets into the original ASCII embedded C#. A refusal
# message or explanatory comment must not satisfy an executable-order check.
LEXEMES = re.compile(
    r'//[^\n]*|/\*.*?\*/|@"(?:""|[^"])*"|"(?:\\.|[^"\\])*"|\'(?:\\.|[^\'\\])*\'',
    re.S,
)


def mask(text, literals=True):
    def replace(match):
        value = match.group()
        if not literals and not value.startswith(("//", "/*")):
            return value
        return "".join("\n" if char == "\n" else " " for char in value)
    return LEXEMES.sub(replace, text)


def block(signature, text=CS):
    """Extract a unique declaration body without counting literal/comment braces."""
    code = mask(text)
    start = code.index(signature)
    if code.find(signature, start + len(signature)) != -1:
        raise AssertionError(f"Ambiguous declaration: {signature}")
    opening = code.index("{", start)
    depth = 1
    for end in range(opening + 1, len(code)):
        depth += (code[end] == "{") - (code[end] == "}")
        if depth == 0:
            return text[start:end + 1]
    raise AssertionError(f"Unbalanced body: {signature}")


def compact(text, literals=False):
    return re.sub(r"\s+", "", mask(text, literals=literals))


def assert_ordered(test, text, *fragments):
    code = compact(text, literals=True)
    after = 0
    for fragment in fragments:
        offset = code.find(fragment, after)
        test.assertNotEqual(offset, -1, f"Missing ordered executable fragment {fragment!r} after offset {after}")
        after = offset + len(fragment)


class RootedMenuItemSourceContracts(unittest.TestCase):
    def test_extractor_ignores_comments_strings_and_character_braces(self):
        snippet = "void First(){/* } */ var x=\"}\"; var y='}'; // }\n Next(); } void Last(){}"
        first = block("void First()", snippet)
        self.assertIn("Next();", compact(first, literals=True))
        self.assertNotIn("Last", first)
        self.assertEqual(compact('/* gone */ @"a""b" // gone\n Run();'), '@"a""b"Run();')
        assert_ordered(self, first, "varx=", "vary=", "Next();")
        with self.assertRaises(AssertionError):
            block("void A()", "void A(){} void A(){}")

    def test_item_facts_require_typed_values_and_retain_only_fixed_name_classification(self):
        facts = block("sealed class BrowserItemFacts")
        constructor = block("public BrowserItemFacts(", facts)
        for token in ("!(name is string)", "((string)name).Length>4096", "!(enabled is bool)", "!(offscreen is bool)", "children==null"):
            self.assertIn(token, constructor)
        self.assertIn('String.Equals((string)name,"Bowser native context permission probe",StringComparison.Ordinal)', constructor)
        self.assertIn("readonly bool FixtureMatch,Enabled,Offscreen", facts)
        self.assertIn("readonly BrowserChildEdges Children", facts)
        semantic = constructor.split('Semantic="', 1)[1]
        for token in ('"supported-match"', '"supported-nonmatch"', "Enabled?", "Offscreen?", "children.Semantic"):
            self.assertIn(token, semantic)
        for forbidden in ("(string)name", "JsonText(name)", "GetHashCode(", "SHA256", "public string Name", "public readonly string Name"):
            self.assertNotIn(forbidden, semantic if forbidden not in ("public string Name", "public readonly string Name") else facts)

    def test_live_item_capture_uses_no_default_name_and_boolean_reads(self):
        capture = block("static BrowserItemFacts CaptureBrowserItemFacts(")
        for property_name in ("NameProperty", "IsEnabledProperty", "IsOffscreenProperty"):
            self.assertIn(f"node.GetCurrentPropertyValue(AutomationElement.{property_name},true)", capture)
        assert_ordered(self, capture, "fence();", "objectname=node.GetCurrentPropertyValue(", "fence();",
                       "objectenabled=node.GetCurrentPropertyValue(", "fence();",
                       "objectoffscreen=node.GetCurrentPropertyValue(", "fence();",
                       "CaptureBrowserChildEdges(", "Same(Runtime(node),id)", "newBrowserItemFacts(")
        self.assertIn("catch{throw new InvalidOperationException(", capture)
        for forbidden in (".Current", "returnnull", "continue;"):
            self.assertNotIn(forbidden, compact(capture, literals=True))
        self.assertNotRegex(compact(capture, literals=True), r"\.(Name|IsEnabled|IsOffscreen)\b")

    def test_ordered_child_edges_are_immutable_and_bind_each_exact_parent(self):
        edges = block("sealed class BrowserChildEdges")
        constructor = block("public BrowserChildEdges(", edges)
        for token in ("readonly int[][] ids,parents", "children.Count!=observedParents.Count", "children.Count>127",
                      "!Same(parent,owner)", "Same(child,owner)", "!seen.Add(JsonIds(child))",
                      "ids[n]=(int[])child.Clone()", "parents[n]=(int[])parent.Clone()"):
            self.assertIn(token, edges)
        self.assertIn("for(int n=0;n<ids.Length;n++)", constructor)
        self.assertIn("value.Append(JsonIds(ids[n])).Append('@').Append(JsonIds(parents[n]))", constructor)
        self.assertIn("return (int[])ids[n].Clone()", block("public int[] Child(", edges))
        self.assertIn("return (int[])parents[n].Clone()", block("public int[] Parent(", edges))
        for forbidden in (".Sort(", "OrderBy(", "Array.Sort(", "HashSet<int[]>"):
            self.assertNotIn(forbidden, compact(edges, literals=True))

    def test_raw_child_reader_is_fenced_bounded_and_never_infers_absence_on_failure(self):
        capture = block("static BrowserChildEdges CaptureBrowserChildEdges<T>(")
        for guard in ("node==null", "owner==null", "first==null", "next==null", "parent==null", "runtime==null", "fence==null"):
            self.assertIn(guard, capture)
        assert_ordered(self, capture, "fence();if(!SameOptional(runtime(node),owner))", "fence();Tchild=first(node);fence();", "while(child!=null)",
                       "children.Count>=127", "runtime(child)", "parent(child)", "runtime(p)",
                       "!Same(parentId,owner)", "!seen.Add(JsonIds(id))", "children.Add((int[])id.Clone())",
                       "parents.Add((int[])parentId.Clone())", "varagain=parent(child)",
                       "!SameOptional(runtime(again),owner)", "!SameOptional(runtime(child),id)", "child=next(child);fence();",
                       "fence();if(!SameOptional(runtime(node),owner))", "fence();",
                       "returnnewBrowserChildEdges(owner,children,parents)")
        for forbidden in ("catch", "return null", "continue;", "ControlType.MenuItem", ".Realize("):
            self.assertNotIn(forbidden, compact(capture, literals=True))

    def test_unsupported_hwnd_stays_distinct_and_supported_row_predicate_stays_strict(self):
        witness = block("sealed class BrowserNodeWitness")
        self.assertIn('NativeToken=nativeUnknown?"not-supported":"integer:"+Number(native)', witness)
        self.assertIn("public readonly BrowserItemFacts Item", witness)
        self.assertIn('+(item==null?"":":"+item.Semantic)', witness)
        row = block("static bool IsBrowserRow(")
        self.assertEqual(compact(row, literals=True),
                         "staticboolIsBrowserRow(BrowserNodeWitnessnode,inttype){returnnode!=null&&!node.NativeUnknown&&!node.BoundsUnknown&&node.Type==type;}")
        factory = block("static BrowserNodeWitness CreateBrowserWitness(")
        self.assertIn("type!=ControlType.Pane.Id&&type!=ControlType.MenuBar.Id&&type!=ControlType.Menu.Id&&type!=ControlType.MenuItem.Id", factory)
        self.assertIn("depth==0||parent==null||!patternsAbsent", factory)
        contained = block("static bool ContainedBrowserBounds(")
        for token in ("Finite(bounds.Right)", "Finite(bounds.Bottom)", "bounds.Width>0", "bounds.Height>0",
                      "bounds.Left>=popup.Left", "bounds.Top>=popup.Top", "bounds.Right<=popup.Right", "bounds.Bottom<=popup.Bottom"):
            self.assertIn(token, contained)
        self.assertNotIn("ControlType.Separator.Id", factory)

    def test_every_captured_menuitem_binds_facts_including_supported_hwnd_items(self):
        capture = block("static BrowserNodeWitness CaptureBrowserNode(")
        binding = "if(proof.Type==ControlType.MenuItem.Id)proof=BindBrowserItemFacts(proof,CaptureBrowserItemFacts(node,id,fence));"
        self.assertIn(binding, capture)
        self.assertNotIn("if(proof.NativeUnknown&&proof.Type==ControlType.MenuItem.Id)", compact(capture, literals=True))
        assert_ordered(self, capture, "CreateBrowserWitness(", binding, "fence();", "!Same(Runtime(node),id)", "!browser.SameAlive()", "returnproof;")
        bind = block("static BrowserNodeWitness BindBrowserItemFacts(")
        for token in ("node.Type!=ControlType.MenuItem.Id", "facts==null", "node.Id(),node.Parent(),node.NativeUnknown,node.BoundsUnknown,facts"):
            self.assertIn(token, bind)
        recheck = block("static BrowserNodeWitness RecheckBrowserNode(")
        assert_ordered(self, recheck, "CaptureBrowserNode(", "!SameBrowserWitness(proof,fresh)", "thrownewInvalidOperationException(")
        self.assertIn("before.Signature==after.Signature", block("static bool SameBrowserWitness("))

    def test_item_token_is_distinct_registered_and_bound_to_immutable_exact_context(self):
        token = block("sealed class RootedItemWitness")
        constructor = block("public RootedItemWitness(", token)
        for guard in ("item.Item==null", "item.Type!=ControlType.MenuItem.Id", "!SameBrowserWitness(ancestry.Start(),item)",
                      "!SameBrowserWitness(ancestry.SingleMenu(),menu)", "!Same(ancestry.Root().Id(),rootId)",
                      "ancestry.Root().NativeUnknown", "ancestry.Root().BoundsUnknown", "ancestry.Root().Native!=popupHandle"):
            self.assertIn(guard, constructor)
        self.assertIn("root=(int[])rootId.Clone()", constructor)
        match = block("public bool Matches(", token)
        for guard in ("Object.ReferenceEquals(invocation,owner)", "popup==popupHandle", "pid==browserPid", "created==browserCreated",
                      "Same(root,rootId)", "SameBrowserWitness(start,item)", "MenuSemantic==menuSemantic"):
            self.assertIn(guard, match)
        semantic = constructor.split('Semantic="', 1)[1]
        for token_value in ("eligible-rooted-item:popup=", "JsonIds(root)", "Number(pid)", "created.ToString(",
                            "item.Signature", "menuSemantic", "chain-sha256=", "Chain"):
            self.assertIn(token_value, semantic)
        for forbidden in ("invocation", "identity", "GetHashCode", "Guid"):
            self.assertNotIn(forbidden, semantic)
        scope = block("sealed class BrowserMenuInvocation")
        self.assertIn("Dictionary<string,RootedMenuWitness> issued=", scope)
        self.assertIn("Dictionary<string,RootedItemWitness> issuedItems=", scope)
        require = block("public RootedItemWitness RequireItem(", scope)
        for guard in ("item.Type!=ControlType.MenuItem.Id", "item.Item==null", "item.BoundsUnknown", "item.Depth<=0", "item.Parent()==null",
                      "item.Pid!=(int)browserPid", "proof==null", "issuedItems.TryGetValue(JsonIds(item.Id()),out registered)",
                      "Object.ReferenceEquals(registered,proof)", "InventorySignature(proof.Menu,ProofFor(proof.Menu))",
                      "proof.Matches(identity,item,root,popup,browserPid,browserCreated,menuSemantic)"):
            self.assertIn(guard, require)
        self.assertNotIn("issuedItems", block("public bool IsCountedMenu(", scope))
        generic = block("public string InventorySignature(", scope)
        self.assertIn("if(node.Type==ControlType.MenuItem.Id)", generic)
        self.assertIn("if(proof!=null)throw new InvalidOperationException(", generic)
        self.assertIn("return ItemInventorySignature(node,ItemProofFor(node))", generic)

    def test_only_exact_pending_start_is_provisional_and_other_item_ancestors_need_tokens(self):
        walk = block("static BrowserAncestryWitness CountedBrowserAncestry<T>(")
        assert_ordered(self, walk, "fence();varfresh=read(current);fence();", "CountedAncestor(nodes,runtime)",
                       "if(hop==0&&(", "!SameBrowserWitness(expectedStart,fresh)", "!SameBrowserWitness(expectedStart,counted)",
                       "!SameBrowserWitness(counted,fresh)", "eligibility(counted,hop==0)", "CountedParent(nodes,child,runtime)",
                       "current=parent(current)")
        eligibility = block("public void CheckAncestryEligibility(")
        for token in ("pending!=null&&SameBrowserWitness(node,pending)", "node.Type==ControlType.MenuItem.Id", "node.Item==null",
                      "if(!exactPending)RequireItem(node,ItemProofFor(node))",
                      "node.Type==ControlType.Menu.Id&&!(exactPending&&node.NativeUnknown)", "IsCountedMenu(node,ProofFor(node))"):
            self.assertIn(token, eligibility)
        self.assertNotIn("return true", eligibility)
        issuer = block("public RootedItemWitness ProveItem<T>(")
        self.assertIn("(node,first)=>CheckAncestryEligibility(node,first?expectedStart:null)", issuer)
        self.assertNotIn("nodes.Add(", compact(issuer, literals=True))

    def test_item_issuance_seals_rechecked_facts_and_edges_only_after_exact_eligible_chain(self):
        issuer = block("public RootedItemWitness ProveItem<T>(")
        for guard in ("expectedStart.Type!=ControlType.MenuItem.Id", "expectedStart.Item==null", "expectedStart.BoundsUnknown",
                      "expectedStart.Depth<=0", "expectedStart.Parent()==null", "expectedStart.Pid!=(int)browserPid",
                      "!expectedStart.NativeUnknown&&expectedStart.Native!=0&&expectedStart.Native!=popup"):
            self.assertIn(guard, issuer)
        assert_ordered(self, issuer, "issuedItems.ContainsKey(key)", "CountedBrowserAncestry(start,expectedStart,nodes,root,browserPid,popup,read,parent,fence,",
                       "FixtureMenu(ancestry,selectedMenu)", "InventorySignature(menu,ProofFor(menu))",
                       "fence();varsealedStart=read(start);fence();", "!SameBrowserWitness(expectedStart,sealedStart)",
                       "newRootedItemWitness(", "fence();", "issuedItems.Add(key,proof)", "returnproof;")
        selected = block("public BrowserNodeWitness FixtureMenu(")
        for token in ("ancestry.SingleMenu()", "SameBrowserWitness(actual,selected)", "IsCountedMenu(actual,proof)",
                      "actual.NativeUnknown&&proof.Chain!=ancestry.ChainFor(actual)"):
            self.assertIn(token, selected)
        item_signature = block("public string ItemInventorySignature(")
        assert_ordered(self, item_signature, "varregistered=RequireItem(item,proof)", "returnitem.Signature+", "+registered.Semantic")

    def test_fixture_authority_requires_match_enabled_onscreen_raw_leaf_and_contained_geometry(self):
        fixture = block("public System.Windows.Rect RootedFixtureBounds(")
        assert_ordered(self, fixture, "RequireItem(item,proof)", "!item.Item.FixtureMatch", "!item.Item.Enabled", "item.Item.Offscreen",
                       "item.Item.Children.Count!=0", "!ContainedBrowserBounds(item.Bounds,popupBounds)", "returnitem.Bounds;")
        self.assertNotIn("IsBrowserRow(", fixture)
        # Ownership of a nonmatch is deliberately not a leaf-only policy.
        issuer = block("public RootedItemWitness ProveItem<T>(")
        self.assertNotIn("Children.Count", issuer)
        self.assertNotIn("FixtureMatch", issuer)
        self.assertNotIn("Enabled", issuer)
        self.assertNotIn("Offscreen", issuer)

    def test_buffered_rows_bind_exact_immutable_identity_facts_and_observed_edges(self):
        row = block("sealed class BufferedInventoryRow")
        constructor = block("public BufferedInventoryRow(", row)
        for token in ("public readonly int Index,Depth,Pid,Type", "readonly int[] parent,id", "public readonly BrowserChildEdges Children",
                      "children==null", "((browser==null)==(gpu==null))", "parentId.Clone()", "runtime.Clone()",
                      "browser.Pid!=pid", "browser.Type!=type", "browser.Depth!=depth", "!Same(browser.Id(),id)",
                      "!SameOptional(browser.Parent(),parent)", "browser.NativeToken!=native", "browser.BoundsToken!=bounds",
                      "browser.Item.Children.Semantic!=children.Semantic", "children.Count!=0||item!=null||menu!=null"):
            self.assertIn(token, row)
        self.assertIn("browser.Item==null", constructor)
        self.assertIn("return (int[])id.Clone()", block("public int[] Id(", row))
        self.assertIn("return parent==null?null:(int[])parent.Clone()", block("public int[] Parent(", row))

    def test_closure_replays_exact_bfs_order_parent_depth_caps_and_all_queued_descendants(self):
        closure = block("static void ValidateInventoryClosure(")
        for token in ("rows==null||rows.Count==0||rows.Count>128", "new Queue<Tuple<int[],int[],int>>()",
                      "rows[0].Id(),(int[])null,0", "row==null||pending.Count==0", "row.Index!=n+1",
                      "!Same(row.Id(),expected.Item1)", "!SameOptional(row.Parent(),expected.Item2)", "row.Depth!=expected.Item3",
                      "!seen.Add(JsonIds(row.Id()))", "child<row.Children.Count", "!Same(row.Children.Parent(child),row.Id())",
                      "!InventoryCapacity(n+1,pending.Count+1)", "row.Children.Child(child),row.Id(),row.Depth+1", "if(pending.Count!=0)"):
            self.assertIn(token, closure)
        self.assertNotIn("continue", compact(closure, literals=True))
        self.assertNotIn("AppendInventoryNode(", closure)
        self.assertNotIn("ItemProof", closure)

    def test_complete_serializer_revalidates_closure_and_serializes_semantics_not_token_identity(self):
        serializer = block("static string SerializeBufferedInventory(")
        assert_ordered(self, serializer, "ValidateInventoryClosure(rows)", "foreach(varrowinrows)",
                       "invocation.ItemInventorySignature(row.Browser,row.ItemProof)",
                       "invocation.InventorySignature(row.Browser,row.MenuProof)", "row.Children.Semantic", "varinventory=newStringBuilder()",
                       "AppendInventoryNode(", "signatures[n]", "returninventory.ToString();")
        category_pass = block("foreach(var row in rows)", serializer)
        self.assertNotIn("AppendInventoryNode(", category_pass)
        self.assertIn("signatures.Add(proof+", category_pass)
        for token in ("row.MenuProof!=null", "row.ItemProof!=null", "row.Browser.Type==ControlType.MenuItem.Id"):
            self.assertIn(token, serializer)
        for forbidden in ("GetHashCode", "ReferenceEquals", "new RootedItemWitness", "new RootedMenuWitness"):
            self.assertNotIn(forbidden, serializer)
        equality = block("public static bool SameProof(")
        self.assertIn("a.Inventory==b.Inventory", equality)
        digest = block("public static string InventoryDigest(")
        self.assertIn("proof.Inventory", digest)
        self.assertNotIn("GetHashCode", digest)

    def test_live_inventory_proves_each_menuitem_and_reads_raw_children_for_nonmatches_too(self):
        live = block("public static MenuProof ReadMenu(")
        self.assertEqual(live.count("new BrowserMenuInvocation(rootId,popup.ToInt32(),browserPid,browser.Created)"), 1)
        self.assertIn("if(browserNode.Type==ControlType.MenuItem.Id)rootedItem=menuInvocation.ProveItem(e,browserNode,countedMenu,browserNodes,readAncestor,ancestor=>walker.GetParent(ancestor),ancestryFence)", live)
        self.assertIn("return RecheckBrowserNode(ancestor,counted,browserPid,browser,popup,ancestryFence)", live)
        self.assertIn("CaptureBrowserNode(e,browserPid,queued.Item2,queued.Item3,browser,popup,ancestryFence)", live)
        self.assertNotIn("c.Name", live)
        self.assertNotIn("c.IsOffscreen", live)
        # The retained GPU outer comparison is a separate, unchanged category.
        browser_live = live.replace(block("if(gpuLeaf!=null)", live), "")
        self.assertNotIn("c.IsEnabled", browser_live)
        fixture = block("if(browserNode!=null&&browserNode.Type==ControlType.MenuItem.Id&&browserNode.Item.FixtureMatch)", live)
        self.assertIn("RootedFixtureBounds(browserNode,rootedItem,", fixture)
        self.assertIn("menuId=rootedItem.Menu.Id()", fixture)
        self.assertNotIn("CaptureBrowserChildEdges(", fixture)
        assert_ordered(self, live, "menuInvocation.ProveItem(", "menuInvocation.RootedFixtureBounds(",
                       "enumeratedEdges=CaptureBrowserChildEdges(", "n=>children.Add(n)",
                       "bufferedRows.Add(newBufferedInventoryRow(", "enumeratedEdges)",
                       "foreach(varchildinchildren)", "queue.Enqueue(Tuple.Create(child,queued.Item2+1,currentId))")

    def test_live_serialization_waits_for_entire_bfs_queue_exhaustion_and_edge_closure(self):
        live = block("public static MenuProof ReadMenu(")
        bfs = block("while(queue.Count>0)", live)
        self.assertIn("new List<BufferedInventoryRow>()", live)
        self.assertIn("bufferedRows.Add(new BufferedInventoryRow(", bfs)
        self.assertNotIn("AppendInventoryNode(", live)
        self.assertNotIn("SerializeBufferedInventory(", bfs)
        self.assertNotIn("return new MenuProof", bfs)
        self.assertNotIn("continue;", compact(bfs, literals=True))
        assert_ordered(self, live, "while(queue.Count>0)", "queue.Enqueue(Tuple.Create(child,queued.Item2+1,currentId))",
                       "ancestryFence();ValidateInventoryClosure(bufferedRows)", "SerializeBufferedInventory(bufferedRows,menuInvocation)",
                       "ancestryFence();", "!UniqueBrowserFixtureCounts(menus,items)", "returnnewMenuProof")
        self.assertEqual(live.count("SerializeBufferedInventory("), 1)
        self.assertIn("Inventory=sealedInventory", live)

    def test_all_live_root_walks_require_registered_unknown_menu_and_item_ancestors(self):
        live = block("public static MenuProof ReadMenu(")
        self.assertEqual(live.count("CountedBrowserAncestry("), 1)
        self.assertIn("ancestryFence,(node,first)=>menuInvocation.CheckAncestryEligibility(node,null)", live)
        menu_issuer = block("public RootedMenuWitness ProveMenu<T>(")
        item_issuer = block("public RootedItemWitness ProveItem<T>(")
        for issuer in (menu_issuer, item_issuer):
            self.assertIn("(node,first)=>CheckAncestryEligibility(node,first?expectedStart:null)", issuer)
        for declaration in ("sealed class BrowserItemFacts", "sealed class BrowserChildEdges", "sealed class RootedItemWitness",
                            "static BrowserChildEdges CaptureBrowserChildEdges<T>(", "public RootedItemWitness ProveItem<T>(",
                            "static void ValidateInventoryClosure(", "static string SerializeBufferedInventory("):
            body = compact(block(declaration), literals=True)
            for forbidden in ("keybd_event(", "mouse_event(", ".Invoke(", ".Select(", ".Expand(", ".Realize(", "SetFocus(", "SetForegroundWindow("):
                self.assertNotIn(forbidden, body, declaration)

    def test_managed_item_matrix_is_wired_but_source_pass_does_not_execute_it(self):
        outer = block("public static void CheckBrowserStructurePolicy(")
        self.assertIn("CheckRootedItemPolicy();", outer)
        runner = (ROOT / "benchmarks/scripts/test-native-menu-helpers.ps1").read_text(encoding="utf-8")
        self.assertIn("GetMethod('CheckBrowserStructurePolicy'", runner)
        self.assertIn("$null=$browserPolicy.Invoke($null,@())", runner)
        self.assertIn("native_behavior=NOT_RUN", runner)
        matrix = block("static void CheckRootedItemPolicy(")
        for predicate in ("new BrowserItemFacts(", "RequireItem(", "ProveItem(", "ItemProofFor(", "InventorySignature(",
                          "CaptureBrowserChildEdges(", "new BufferedInventoryRow(", "SerializeBufferedInventory(",
                          "ValidateInventoryClosure(", "SameProof(", "InventoryDigest(", "UniqueBrowserFixtureCounts"):
            # The fixture/count predicate lives in the harness called by the matrix.
            target = matrix + block("sealed class SyntheticItemState") if predicate == "UniqueBrowserFixtureCounts" else matrix
            self.assertIn(predicate, target)
        for forbidden in ("GetCurrentPropertyValue(", "keybd_event(", "mouse_event(", "ReadMenu(", "NativeReady(", "RootFence(",
                          "GetTickCount64(", "FileStream(", "File.Write", "Process.Start("):
            self.assertNotIn(forbidden, compact(matrix, literals=True))

    def test_managed_harness_feeds_actual_issuer_child_observer_fixture_and_complete_serializer(self):
        harness = block("sealed class SyntheticItemState")
        self.assertIn("return CaptureBrowserChildEdges(", block("public BrowserChildEdges Edges(", harness))
        self.assertIn("return Invocation.ProveItem(node,node.Value,Chain.Menu.Value,Chain.Nodes", block("public RootedItemWitness Seal(", harness))
        self.assertIn("foreach(var node in Order)", block("public List<BufferedInventoryRow> Rows(", harness))
        self.assertIn("new BufferedInventoryRow(", block("public List<BufferedInventoryRow> Rows(", harness))
        self.assertIn("return SerializeBufferedInventory(Rows(),Invocation)", block("public string Inventory(", harness))
        fixture = block("public void CheckFixture(", harness)
        for predicate in ("node.Value.Item.FixtureMatch", "Invocation.RootedFixtureBounds(", "matches++", "!UniqueBrowserFixtureCounts(1,matches)"):
            self.assertIn(predicate, fixture)
        matrix = block("static void CheckRootedItemPolicy(")
        for token in ("new SyntheticItemState(true,false)", "new SyntheticItemState(true,true)", "new SyntheticItemState(false,false)",
                      "Object.ReferenceEquals(token,fresh.Invocation.ItemProofFor(fresh.Chain.Item.Value))",
                      "!SameProof(baseline,freshProof)", "InventoryDigest(baseline)!=InventoryDigest(freshProof)",
                      "completed!=null", "incompleteProof!=null", "parents!=0||reads!=(expected==null?0:1)",
                      "for(int failAt=1;failAt<=22;failAt++)", "new string('x',4097)"):
            self.assertIn(token, matrix)

    def test_managed_matrix_names_refusal_categories_without_claiming_live_race_coverage(self):
        matrix = block("static void CheckRootedItemPolicy(")
        for label in ("item-unregistered", "item-unproved-serialization", "item-pending-inventory", "item-not-supported-fixture",
                      "item-duplicate-issuance", "item-replayed-token", "item-menu-token-cross-use", "item-token-other-category",
                      "item-forged-unregistered-reference", "item-name-unavailable-kind-or-cap", "item-enabled-unavailable-kind",
                      "item-offscreen-unavailable-kind", "item-valid-fixture-plus-unavailable-peer", "item-stale-signature",
                      "item-wrong-root-browser-menu", "item-first-read-substitution", "item-getter-edge-or-late-seal-failure",
                      "item-before-after-registration-fence", "item-token-not-descendant-completion", "item-late-child-after-seal",
                      "item-issued-parent-refused-queued-descendant",
                      "item-unproved-item-ancestor", "item-ancestor-facts-changed", "item-fixture-facts-or-count", "item-row-order",
                      "item-child-self-cycle", "item-child-wrong-parent", "item-direct-child-cap", "item-child-getter-or-deadline",
                      "item-duplicate-direct-child", "item-same-runtime-ordered-child-change",
                      "item-child-edge-changed-before-sibling-advance", "item-raw-repeated-sibling-cycle", "item-raw-child-reader-cap",
                      "item-pattern-unavailable-or-present", "item-not-separator-authority", "item-required-typed-property-or-overflow",
                      "item-complete-row-cap"):
            self.assertIn(f'"{label}"', matrix)
        for mutation in ("supported-hwnd-swap", "bounds", "runtime", "parent", "classification", "enabled", "offscreen", "child-edges",
                         "wrong-root", "wrong-popup", "wrong-browser", "unproved-menu", "wrong-eligible-menu", "root", "ancestor", "peer-item",
                         "changed-start", "changed-counted-start", "null-start", "getter", "parent-getter", "null-parent",
                         "skipped-parent", "cyclic-parent", "changed-counted-parent", "foreign-parent", "late-facts", "late-child", "disabled", "child", "duplicate", "missing",
                         "parent-before-sibling", "runtime-before-sibling"):
            self.assertIn(f'"{mutation}"', matrix)


if __name__ == "__main__":
    unittest.main()
