Attribute VB_Name = "GussetExport"
'==============================================================================
' GussetExport.bas  -  SolidWorks feature-tree exporter for Gusset (ADR 0042)
'
' UNVERIFIED - not yet run against SolidWorks. Written against the SolidWorks
' 2024 API help (help.solidworks.com/2024/english/api). Read-only: it never
' changes the model. Writes <part>.gtree.json next to the part (ADR 0042, v1).
'
' Late bound on purpose (all API objects are "Object"), so no type-library
' reference is needed. Enum values are written as numeric constants below;
' check each against the Object Browser (SldWorks / swconst) before trusting.
'
' Units: the API is in metres / radians; the file is in mm / degrees.
' Axes:  SolidWorks is Y-up; the file is Z-up: x' = x, y' = -z, z' = y
'        (applied to every world point and direction).
'==============================================================================
Option Explicit

Private Const EXPORTER As String = "plugins/solidworks 0.1.0"
Private Const PI As Double = 3.14159265358979

' swDocumentTypes_e
Private Const SW_DOC_PART As Long = 1
' swSketchSegments_e
Private Const SW_SEG_LINE As Long = 0
Private Const SW_SEG_ARC As Long = 1
' swEndConditions_e
Private Const SW_END_BLIND As Long = 0
Private Const SW_END_MIDPLANE As Long = 6
' swRevolveType_e
Private Const SW_REV_ONE_DIR As Long = 0
Private Const SW_REV_MIDPLANE As Long = 1
' swConstraintType_e (from memory; VERIFY in Object Browser)
Private Const SW_CON_HORIZONTAL As Long = 1
Private Const SW_CON_VERTICAL As Long = 2
Private Const SW_CON_TANGENT As Long = 3
Private Const SW_CON_PARALLEL As Long = 4
Private Const SW_CON_PERPENDICULAR As Long = 5
' swSketchRelationFilterType_e swAll, swUserPreferenceIntegerValue_e swUnitsLinear (VERIFY)
Private Const SW_REL_FILTER_ALL As Long = 0
Private Const SW_PREF_UNITS_LINEAR As Long = 0

Private swApp As Object
Private swModel As Object
Private gFeat As String          ' JSON text of the features array body
Private gFeatCount As Long
Private gWarn As String
Private gHaveBody As Boolean
Private gDocMm As Double         ' document length unit in mm (for equations)
Private gDone As Collection      ' sketch feature names already exported
Private gSegMap As Collection    ' "sketch|segKey" -> curve id
Private gDepDim As Collection    ' "D1@Feature" -> global variable name
Private gGlobals As String       ' "|name|name|" of global variables
Private gSkipPlanes As Long

' per-sketch point table (mm, sketch coordinates)
Private pX() As Double, pY() As Double, pC() As Boolean
Private nP As Long

'------------------------------------------------------------------ entry point
Sub main()
    Set swApp = Application.SldWorks
    Set swModel = swApp.ActiveDoc
    If swModel Is Nothing Then MsgBox "Open a part first.": Exit Sub
    If swModel.GetType <> SW_DOC_PART Then MsgBox "GussetExport works on parts only.": Exit Sub

    Set gDone = New Collection
    Set gSegMap = New Collection
    Set gDepDim = New Collection
    gFeat = "": gFeatCount = 0: gWarn = "": gHaveBody = False: gSkipPlanes = 0: gGlobals = ""
    gDocMm = DocLengthMm()

    Dim params As String
    params = ExportParameters()

    Dim feat As Object
    Set feat = swModel.FirstFeature
    Do While Not feat Is Nothing
        ExportFeature feat
        Set feat = feat.GetNextFeature
    Loop

    Dim doc As String
    doc = "{" & vbCrLf & _
        "  ""format"": ""gusset-feature-tree""," & vbCrLf & _
        "  ""version"": 1," & vbCrLf & _
        "  ""source"": { ""app"": ""solidworks"", ""app_version"": " & J(CStr(swApp.RevisionNumber)) & _
        ", ""exporter"": " & J(EXPORTER) & ", ""document"": " & J(DocName()) & " }," & vbCrLf & _
        "  ""parameters"": [" & params & "]," & vbCrLf & _
        "  ""features"": [" & vbCrLf & gFeat & vbCrLf & "  ]," & vbCrLf & _
        "  ""bodies"": [" & ExportBodies() & "]" & vbCrLf & "}" & vbCrLf

    Dim path As String
    path = OutPath()
    Dim f As Integer
    f = FreeFile
    Open path For Output As #f
    Print #f, doc;
    Close #f
    Dim msg As String
    msg = "Wrote " & path & vbCrLf & gFeatCount & " feature entries."
    If Len(gWarn) > 0 Then msg = msg & vbCrLf & vbCrLf & "Warnings:" & vbCrLf & gWarn
    MsgBox msg
End Sub

'------------------------------------------------------------------ features
Private Sub ExportFeature(ByVal feat As Object)
    Dim t As String
    t = feat.GetTypeName2
    If feat.IsSuppressed Then Exit Sub
    Select Case t
        Case "ProfileFeature"
            ExportSketchOnce feat
        Case "Extrusion", "Cut"
            ExportSubSketches feat
            AddOrUnsupported feat, ExportExtrude(feat, (t = "Cut")), t
        Case "Revolution", "RevCut"
            ExportSubSketches feat
            AddOrUnsupported feat, ExportRevolve(feat, (t = "RevCut")), t
        Case "Fillet"
            AddOrUnsupported feat, ExportFillet(feat), t
        Case "Chamfer"
            AddOrUnsupported feat, ExportChamfer(feat), t
        Case "OriginProfileFeature", "MaterialFolder", "CommentsFolder", "FavoriteFolder", _
             "SelectionSetFolder", "SensorFolder", "DocsFolder", "HistoryFolder", "EnvFolder", _
             "DetailCabinet", "SurfaceBodyFolder", "SolidBodyFolder", "EqnFolder", _
             "LiveSnapshotFolder", "InkMarkupFolder", "CoordSys", "RefAxis"
            ' not geometry features: not listed
        Case "RefPlane"
            gSkipPlanes = gSkipPlanes + 1          ' first three are the default planes
            If gSkipPlanes > 3 Then AddOrUnsupported feat, "", t
        Case Else
            AddOrUnsupported feat, "", t
    End Select
End Sub

Private Sub AddOrUnsupported(ByVal feat As Object, ByVal json As String, ByVal t As String)
    If Len(json) = 0 Then
        json = "    { ""type"": ""unsupported"", ""name"": " & J(feat.Name) & _
               ", ""original_type"": " & J(t) & " }"
    End If
    If gFeatCount > 0 Then gFeat = gFeat & "," & vbCrLf
    gFeat = gFeat & json
    gFeatCount = gFeatCount + 1
End Sub

Private Sub ExportSubSketches(ByVal feat As Object)
    ' a sketch consumed by a feature is a sub-feature, not returned by GetNextFeature
    Dim sub1 As Object
    Set sub1 = feat.GetFirstSubFeature
    Do While Not sub1 Is Nothing
        If sub1.GetTypeName2 = "ProfileFeature" Then ExportSketchOnce sub1
        Set sub1 = sub1.GetNextSubFeature
    Loop
End Sub

Private Function FirstSketchName(ByVal feat As Object) As String
    Dim sub1 As Object
    Set sub1 = feat.GetFirstSubFeature
    Do While Not sub1 Is Nothing
        If sub1.GetTypeName2 = "ProfileFeature" Then FirstSketchName = sub1.Name: Exit Function
        Set sub1 = sub1.GetNextSubFeature
    Loop
End Function

Private Sub ExportSketchOnce(ByVal feat As Object)
    Dim d As Boolean
    On Error Resume Next
    gDone.Add feat.Name, feat.Name
    d = (Err.Number <> 0)         ' duplicate key -> already exported
    Err.Clear
    On Error GoTo 0
    If d Then Exit Sub
    If feat.IsSuppressed Then Exit Sub
    AddOrUnsupported feat, ExportSketch(feat), "ProfileFeature"
End Sub

'------------------------------------------------------------------ sketch
Private Function ExportSketch(ByVal feat As Object) As String
    On Error GoTo Fail
    Dim sk As Object, inv As Object
    Set sk = feat.GetSpecificFeature2
    Set inv = sk.ModelToSketchTransform.Inverse        ' sketch -> model
    Dim o As Variant, xa As Variant, ya As Variant, nrm As Variant
    o = XfPoint(inv, 0, 0, 0)
    xa = XfVec(inv, 1, 0, 0)
    ya = XfVec(inv, 0, 1, 0)
    nrm = XfVec(inv, 0, 0, 1)                           ' model-space sketch normal

    nP = 0
    ReDim pX(0 To 63): ReDim pY(0 To 63): ReDim pC(0 To 63)

    Dim curves As String, nl As Long, na As Long, nc As Long, id As String
    Dim segs As Variant, i As Long, seg As Object, tp As Long, cons As Boolean
    Dim sp As Long, ep As Long, cp As Long, ap As Long, bp As Long, tmp As Long
    Dim st As Object, en As Object, cen As Object, r As Double
    segs = sk.GetSketchSegments
    If IsArray(segs) Then
        For i = LBound(segs) To UBound(segs)
            Set seg = segs(i)
            tp = seg.GetType
            cons = seg.ConstructionGeometry
            If tp = SW_SEG_LINE Then
                nl = nl + 1: id = "l" & nl
                sp = PointOf(seg.GetStartPoint2, cons)
                ep = PointOf(seg.GetEndPoint2, cons)
                curves = curves & CurveSep(curves) & "        { ""id"": " & J(id) & ", ""type"": ""line"", ""start"": " & _
                    J("p" & sp) & ", ""end"": " & J("p" & ep) & ", ""construction"": " & JB(cons) & " }"
                MapSeg feat.Name, seg, id
            ElseIf tp = SW_SEG_ARC Then
                Set st = seg.GetStartPoint2: Set en = seg.GetEndPoint2: Set cen = seg.GetCenterPoint2
                cp = PointOf(cen, cons)
                r = Sqr((st.X - cen.X) ^ 2 + (st.Y - cen.Y) ^ 2) * 1000#
                If CBool(seg.IsCircle) Then
                    nc = nc + 1: id = "c" & nc
                    curves = curves & CurveSep(curves) & "        { ""id"": " & J(id) & ", ""type"": ""circle"", ""center"": " & _
                        J("p" & cp) & ", ""radius"": " & N(r) & ", ""construction"": " & JB(cons) & " }"
                Else
                    na = na + 1: id = "a" & na
                    ap = PointOf(st, cons): bp = PointOf(en, cons)
                    If Not ArcIsCCW(seg, nrm) Then tmp = ap: ap = bp: bp = tmp
                    curves = curves & CurveSep(curves) & "        { ""id"": " & J(id) & ", ""type"": ""arc"", ""center"": " & _
                        J("p" & cp) & ", ""start"": " & J("p" & ap) & ", ""end"": " & J("p" & bp) & _
                        ", ""construction"": " & JB(cons) & " }"
                End If
                MapSeg feat.Name, seg, id
            Else
                gWarn = gWarn & "  " & feat.Name & ": sketch segment type " & tp & " skipped" & vbCrLf
            End If
        Next i
    End If
    ' free sketch points (endpoints and centres are found again and de-duplicated)
    Dim pts As Variant
    pts = sk.GetSketchPoints2
    If IsArray(pts) Then
        For i = LBound(pts) To UBound(pts)
            tmp = PointOf(pts(i), False)
        Next i
    End If

    Dim js As String, k As Long
    js = "    { ""type"": ""sketch"", ""name"": " & J(feat.Name) & "," & vbCrLf & _
         "      ""plane"": { ""origin"": " & JPt(W3(o, True)) & ", ""x_axis"": " & JPt(W3(xa, False)) & _
         ", ""y_axis"": " & JPt(W3(ya, False)) & " }," & vbCrLf & "      ""points"": [" & vbCrLf
    For k = 1 To nP
        js = js & "        { ""id"": " & J("p" & k) & ", ""x"": " & N(pX(k)) & ", ""y"": " & N(pY(k)) & _
             ", ""construction"": " & JB(pC(k)) & " }" & IIf(k < nP, ",", "") & vbCrLf
    Next k
    js = js & "      ]," & vbCrLf & "      ""curves"": [" & vbCrLf & curves & vbCrLf & "      ]," & vbCrLf & _
         "      ""constraints"": [" & vbCrLf & SketchConstraints(sk, feat.Name) & "      ] }"
    ExportSketch = js
    Exit Function
Fail:
    gWarn = gWarn & "  " & feat.Name & ": sketch not exported (" & Err.Description & ")" & vbCrLf
    ExportSketch = ""
End Function

Private Function CurveSep(ByVal s As String) As String
    If Len(s) > 0 Then CurveSep = "," & vbCrLf
End Function

' index (1-based) of a sketch point, adding it when new. Coordinates in mm.
Private Function PointOf(ByVal sp As Object, ByVal cons As Boolean) As Long
    Dim x As Double, y As Double, i As Long
    x = sp.X * 1000#: y = sp.Y * 1000#
    For i = 1 To nP
        If Abs(pX(i) - x) < 0.0001 And Abs(pY(i) - y) < 0.0001 Then
            If Not cons Then pC(i) = False
            PointOf = i
            Exit Function
        End If
    Next i
    nP = nP + 1
    If nP > UBound(pX) Then
        ReDim Preserve pX(0 To nP * 2): ReDim Preserve pY(0 To nP * 2): ReDim Preserve pC(0 To nP * 2)
    End If
    pX(nP) = x: pY(nP) = y: pC(nP) = cons
    PointOf = nP
End Function

' Arc direction: SolidWorks arcs run counter-clockwise about the curve's circle axis;
' compare that axis with the sketch normal (both in model space). UNVERIFIED.
Private Function ArcIsCCW(ByVal seg As Object, ByVal nrm As Variant) As Boolean
    On Error GoTo Fail
    Dim cp As Variant
    cp = seg.GetCurve.CircleParams            ' cx,cy,cz, ax,ay,az, radius
    ArcIsCCW = ((cp(3) * nrm(0) + cp(4) * nrm(1) + cp(5) * nrm(2)) >= 0)
    Exit Function
Fail:
    gWarn = gWarn & "  arc direction unknown, assumed counter-clockwise" & vbCrLf
    ArcIsCCW = True
End Function

Private Sub MapSeg(ByVal sketchName As String, ByVal seg As Object, ByVal id As String)
    Dim k As String
    k = SegKey(seg)
    If Len(k) = 0 Then Exit Sub
    On Error Resume Next
    gSegMap.Add id, sketchName & "|" & k
    On Error GoTo 0
End Sub

Private Function SegKey(ByVal ent As Object) As String
    On Error GoTo Fail
    Dim a As Variant
    a = ent.GetID                             ' array: sketch id, entity id
    SegKey = CStr(a(0)) & "_" & CStr(a(1))
    Exit Function
Fail:
    SegKey = ""
End Function

Private Function MapGet(ByVal sketchName As String, ByVal key As String) As String
    On Error Resume Next
    MapGet = gSegMap.Item(sketchName & "|" & key)
    If Err.Number <> 0 Then MapGet = ""
    Err.Clear
End Function

' relations of the SolidWorks types that correspond to the ADR's; others are skipped
' (dimension-driven constraints and point relations are not exported in 0.1.0).
Private Function SketchConstraints(ByVal sk As Object, ByVal sketchName As String) As String
    On Error GoTo Fail
    Dim rels As Variant, i As Long, j As Long, out As String, ty As String
    Dim ents As Variant, ids As String, ok As Boolean, cid As String
    rels = sk.RelationManager.GetRelations(SW_REL_FILTER_ALL)
    If Not IsArray(rels) Then Exit Function
    For i = LBound(rels) To UBound(rels)
        Select Case rels(i).GetRelationType
            Case SW_CON_HORIZONTAL: ty = "horizontal"
            Case SW_CON_VERTICAL: ty = "vertical"
            Case SW_CON_TANGENT: ty = "tangent"
            Case SW_CON_PARALLEL: ty = "parallel"
            Case SW_CON_PERPENDICULAR: ty = "perpendicular"
            Case Else: ty = ""
        End Select
        If Len(ty) > 0 Then
            ents = rels(i).GetEntities
            ids = "": ok = IsArray(ents)
            If ok Then
                For j = LBound(ents) To UBound(ents)
                    cid = MapGet(sketchName, SegKey(ents(j)))
                    If Len(cid) = 0 Then ok = False: Exit For
                    ids = ids & IIf(Len(ids) > 0, ", ", "") & J(cid)
                Next j
            End If
            If ok Then
                out = out & IIf(Len(out) > 0, "," & vbCrLf, "") & "        { ""type"": " & J(ty) & ", ""entities"": [" & ids & "] }"
            End If
        End If
    Next i
    If Len(out) > 0 Then out = out & vbCrLf
    SketchConstraints = out
    Exit Function
Fail:
    gWarn = gWarn & "  " & sketchName & ": relations skipped (" & Err.Description & ")" & vbCrLf
    SketchConstraints = ""
End Function

'------------------------------------------------------------------ extrude / revolve
Private Function ExportExtrude(ByVal feat As Object, ByVal isCut As Boolean) As String
    On Error GoTo Fail
    Dim fd As Object, skn As String
    skn = FirstSketchName(feat)
    If Len(skn) = 0 Then GoTo Fail
    Set fd = feat.GetDefinition
    Dim ec As Long, d1 As Double, d2 As Double, dist As Double, sym As Boolean
    ec = fd.GetEndCondition(True)
    d1 = fd.GetDepth(True) * 1000#
    If ec = SW_END_MIDPLANE Then
        sym = True: dist = d1
    ElseIf ec = SW_END_BLIND Then
        If fd.BothDirections Then
            If fd.GetEndCondition(False) <> SW_END_BLIND Then GoTo Fail
            d2 = fd.GetDepth(False) * 1000#
            If Abs(d1 - d2) > 0.000001 Then GoTo Fail      ' asymmetric: unsupported
            sym = True: dist = d1 + d2
        Else
            sym = False: dist = d1
        End If
    Else
        GoTo Fail                                            ' up to face, through all, ...
    End If
    If fd.ReverseDirection Then dist = -dist
    Dim op As String
    If isCut Then
        op = "cut"
    ElseIf gHaveBody And fd.Merge Then
        op = "join"
    Else
        op = "new"
    End If
    If Not isCut Then gHaveBody = True
    Dim js As String, gp As String
    js = "    { ""type"": ""extrude"", ""name"": " & J(feat.Name) & ", ""sketch"": " & J(skn) & _
         ", ""profiles"": ""all"", ""distance"": " & N(dist) & ", ""symmetric"": " & JB(sym)
    gp = DepParam("D1@" & feat.Name)
    If Len(gp) > 0 Then js = js & ", ""distance_parameter"": " & J(gp)
    ExportExtrude = js & ", ""operation"": " & J(op) & " }"
    Exit Function
Fail:
    If Err.Number <> 0 Then gWarn = gWarn & "  " & feat.Name & ": extrude not exported (" & Err.Description & ")" & vbCrLf
    ExportExtrude = ""
End Function

Private Function ExportRevolve(ByVal feat As Object, ByVal isCut As Boolean) As String
    On Error GoTo Fail
    Dim fd As Object, skn As String, accessed As Boolean
    skn = FirstSketchName(feat)
    If Len(skn) = 0 Then GoTo Fail
    Set fd = feat.GetDefinition
    fd.AccessSelections swModel, Nothing
    accessed = True
    Dim ty As Long, ang As Double, sym As Boolean
    ty = fd.Type
    If ty <> SW_REV_ONE_DIR And ty <> SW_REV_MIDPLANE Then GoTo Fail
    sym = (ty = SW_REV_MIDPLANE)
    ang = fd.GetRevolutionAngle(True) * 180# / PI
    If fd.ReverseDirection Then ang = -ang
    Dim axisObj As Object, cid As String
    Set axisObj = fd.Axis
    cid = MapGet(skn, SegKey(axisObj))
    If Len(cid) = 0 Then GoTo Fail                           ' axis is not a sketch line
    fd.ReleaseSelectionAccess: accessed = False
    Dim op As String
    If isCut Then
        op = "cut"
    ElseIf gHaveBody Then
        op = "join"
    Else
        op = "new"
    End If
    If Not isCut Then gHaveBody = True
    ExportRevolve = "    { ""type"": ""revolve"", ""name"": " & J(feat.Name) & ", ""sketch"": " & J(skn) & _
        ", ""profiles"": ""all"", ""axis"": { ""line"": " & J(cid) & " }, ""angle"": " & N(ang) & _
        ", ""symmetric"": " & JB(sym) & ", ""operation"": " & J(op) & " }"
    Exit Function
Fail:
    If Err.Number <> 0 Then gWarn = gWarn & "  " & feat.Name & ": revolve not exported (" & Err.Description & ")" & vbCrLf
    On Error Resume Next
    If accessed Then fd.ReleaseSelectionAccess
    ExportRevolve = ""
End Function

'------------------------------------------------------------------ fillet / chamfer
Private Function ExportFillet(ByVal feat As Object) As String
    On Error GoTo Fail
    Dim fd As Object, accessed As Boolean
    Set fd = feat.GetDefinition
    If fd.IsMultipleRadius Then GoTo Fail                    ' per-edge radii
    fd.AccessSelections swModel, Nothing
    accessed = True
    Dim items As Variant, edges As String, r As Double
    items = fd.GetFilletItems
    edges = EdgeList(items)
    If Len(edges) = 0 Then GoTo Fail
    r = fd.DefaultRadius * 1000#
    fd.ReleaseSelectionAccess: accessed = False
    ExportFillet = "    { ""type"": ""fillet"", ""name"": " & J(feat.Name) & ", ""edges"": [" & edges & "], ""radius"": " & N(r) & " }"
    Exit Function
Fail:
    If Err.Number <> 0 Then gWarn = gWarn & "  " & feat.Name & ": fillet not exported (" & Err.Description & ")" & vbCrLf
    On Error Resume Next
    If accessed Then fd.ReleaseSelectionAccess
    ExportFillet = ""
End Function

Private Function ExportChamfer(ByVal feat As Object) As String
    On Error GoTo Fail
    Dim fd As Object, accessed As Boolean, edges As String, d1 As Double
    Set fd = feat.GetDefinition
    fd.AccessSelections swModel, Nothing
    accessed = True
    edges = EdgeList(fd.GetEdges)                      ' UNVERIFIED member name
    If Len(edges) = 0 Then GoTo Fail
    d1 = fd.GetEdgeChamferDistance(0) * 1000#          ' UNVERIFIED member; first edge's distance
    fd.ReleaseSelectionAccess: accessed = False
    ExportChamfer = "    { ""type"": ""chamfer"", ""name"": " & J(feat.Name) & ", ""edges"": [" & edges & "], ""distance"": " & N(d1) & " }"
    Exit Function
Fail:
    If Err.Number <> 0 Then gWarn = gWarn & "  " & feat.Name & ": chamfer not exported (" & Err.Description & ")" & vbCrLf
    On Error Resume Next
    If accessed Then fd.ReleaseSelectionAccess
    ExportChamfer = ""
End Function

' "{ midpoint: [x,y,z] }" for each edge (mm, Z-up). Anything that is not an edge -> "" (feature unsupported).
Private Function EdgeList(ByVal items As Variant) As String
    On Error GoTo Fail
    If Not IsArray(items) Then Exit Function
    Dim i As Long, e As Object, cp As Variant, cv As Object, t As Double, p As Variant, out As String
    For i = LBound(items) To UBound(items)
        Set e = items(i)
        Set cv = e.GetCurve
        cp = e.GetCurveParams2                     ' [6],[7] = start / end parameter
        t = (cp(6) + cp(7)) / 2#
        p = cv.Evaluate2(t, 0)                     ' first three values: point (m, Y-up)
        out = out & IIf(Len(out) > 0, ", ", "") & "{ ""midpoint"": " & JPt(Conv(p(0), p(1), p(2), True)) & " }"
    Next i
    EdgeList = out
    Exit Function
Fail:
    gWarn = gWarn & "  edge midpoint failed (" & Err.Description & ")" & vbCrLf
    EdgeList = ""
End Function

'------------------------------------------------------------------ parameters (global variables)
Private Function ExportParameters() As String
    On Error GoTo Fail
    Dim em As Object, i As Long, n As Long, out As String, eq As String
    Dim q1 As Long, q2 As Long, nm As String, rhs As String, eqp As Long
    Dim v As Double, kind As String, ex As String, g As String
    Set em = swModel.GetEquationMgr
    n = em.GetCount
    For i = 0 To n - 1
        eq = em.Equation(i)
        q1 = InStr(eq, """"): q2 = InStr(q1 + 1, eq, """")
        eqp = InStr(q2 + 1, eq, "=")
        If q1 > 0 And q2 > q1 And eqp > 0 Then
            nm = Mid$(eq, q1 + 1, q2 - q1 - 1)
            rhs = Trim$(Mid$(eq, eqp + 1))
            If em.GlobalVariable(i) Then
                If Not ParseEq(rhs, v, kind) Then
                    v = em.Value(i) * gDocMm          ' formula: value in document units (UNVERIFIED)
                    kind = "length"
                End If
                If kind = "length" Then ex = N(v) & " mm"
                If kind = "angle" Then ex = N(v) & " deg"
                If kind = "number" Then ex = N(v)
                out = out & IIf(Len(out) > 0, "," & vbCrLf, vbCrLf) & "    { ""name"": " & J(nm) & ", ""expression"": " & J(ex) & _
                    ", ""value"": " & N(v) & ", ""kind"": " & J(kind) & " }"
                gGlobals = gGlobals & "|" & nm & "|"
            ElseIf Len(rhs) > 2 And Left$(rhs, 1) = """" And Right$(rhs, 1) = """" Then
                ' "D1@Boss-Extrude1" = "depth"  -> dimension driven by a global variable
                g = Mid$(rhs, 2, Len(rhs) - 2)
                If InStr(g, """") = 0 Then
                    On Error Resume Next
                    gDepDim.Add g, nm
                    On Error GoTo Fail
                End If
            End If
        End If
    Next i
    If Len(out) > 0 Then out = out & vbCrLf & "  "
    ExportParameters = out
    Exit Function
Fail:
    gWarn = gWarn & "  parameters not exported (" & Err.Description & ")" & vbCrLf
    ExportParameters = ""
End Function

Private Function DepParam(ByVal dimName As String) As String
    On Error Resume Next
    DepParam = gDepDim.Item(dimName)
    If Err.Number <> 0 Then DepParam = ""
    Err.Clear
    If InStr(gGlobals, "|" & DepParam & "|") = 0 Then DepParam = ""
End Function

' "40mm", "1.5 in", "30deg", "12" (bare = document length unit). False for formulas.
Private Function ParseEq(ByVal s As String, ByRef v As Double, ByRef kind As String) As Boolean
    Dim i As Long, c As String, num As String, u As String
    s = LCase$(Trim$(s))
    For i = 1 To Len(s)
        c = Mid$(s, i, 1)
        If (c >= "0" And c <= "9") Or c = "." Or (i = 1 And (c = "-" Or c = "+")) Then
            num = num & c
        Else
            Exit For
        End If
    Next i
    If Len(num) = 0 Or num = "-" Or num = "+" Then Exit Function
    u = Trim$(Mid$(s, Len(num) + 1))
    v = Val(num)                                  ' Val always uses "." (locale independent)
    kind = "length"
    Select Case u
        Case "": v = v * gDocMm
        Case "mm": v = v * 1#
        Case "cm": v = v * 10#
        Case "m": v = v * 1000#
        Case "in", """": v = v * 25.4
        Case "ft": v = v * 304.8
        Case "deg": kind = "angle"
        Case "rad": kind = "angle": v = v * 180# / PI
        Case Else: Exit Function
    End Select
    ParseEq = True
End Function

Private Function DocLengthMm() As Double
    On Error GoTo Fail
    Select Case swModel.Extension.GetUserPreferenceInteger(SW_PREF_UNITS_LINEAR, 0)
        Case 0: DocLengthMm = 1#
        Case 1: DocLengthMm = 10#
        Case 2: DocLengthMm = 1000#
        Case 3: DocLengthMm = 25.4
        Case 4, 5: DocLengthMm = 304.8
        Case Else: DocLengthMm = 1#
    End Select
    Exit Function
Fail:
    DocLengthMm = 1#
End Function

'------------------------------------------------------------------ bodies
Private Function ExportBodies() As String
    On Error GoTo Fail
    Dim bs As Variant, i As Long, b As Object, mp As Variant, bx As Variant, out As String
    Dim lo As Variant, hi As Variant
    bs = swModel.GetBodies2(0, True)               ' swSolidBody, visible only
    If Not IsArray(bs) Then Exit Function
    For i = LBound(bs) To UBound(bs)
        Set b = bs(i)
        mp = b.GetMassProperties(1#)               ' [3] = volume (m^3)
        bx = b.GetBodyBox                          ' xmin,ymin,zmin,xmax,ymax,zmax (m, Y-up)
        ' Y-up -> Z-up: y' = -z reverses the order of the z extent
        lo = Array(bx(0) * 1000#, -bx(5) * 1000#, bx(1) * 1000#)
        hi = Array(bx(3) * 1000#, -bx(2) * 1000#, bx(4) * 1000#)
        out = out & IIf(Len(out) > 0, "," & vbCrLf, vbCrLf) & "    { ""name"": " & J(b.Name) & ", ""volume_mm3"": " & _
            N(mp(3) * 1000000000#) & ", ""bbox_mm"": [" & JPt(lo) & ", " & JPt(hi) & "] }"
    Next i
    If Len(out) > 0 Then out = out & vbCrLf & "  "
    ExportBodies = out
    Exit Function
Fail:
    gWarn = gWarn & "  bodies not exported (" & Err.Description & ")" & vbCrLf
    ExportBodies = ""
End Function

'------------------------------------------------------------------ geometry helpers
Private Function XfPoint(ByVal xf As Object, ByVal x As Double, ByVal y As Double, ByVal z As Double) As Variant
    Dim mu As Object, p As Object
    Set mu = swApp.GetMathUtility
    Set p = mu.CreatePoint(Array(x, y, z))
    XfPoint = p.MultiplyTransform(xf).ArrayData
End Function

Private Function XfVec(ByVal xf As Object, ByVal x As Double, ByVal y As Double, ByVal z As Double) As Variant
    Dim mu As Object, v As Object
    Set mu = swApp.GetMathUtility
    Set v = mu.CreateVector(Array(x, y, z))
    XfVec = v.MultiplyTransform(xf).ArrayData
End Function

' model (m, Y-up) -> file (mm for points / unitless for directions, Z-up)
Private Function Conv(ByVal x As Double, ByVal y As Double, ByVal z As Double, ByVal isPoint As Boolean) As Variant
    Dim s As Double: s = IIf(isPoint, 1000#, 1#)
    Conv = Array(x * s, -z * s, y * s)
End Function

Private Function W3(ByVal a As Variant, ByVal isPoint As Boolean) As Variant
    W3 = Conv(a(0), a(1), a(2), isPoint)
End Function

'------------------------------------------------------------------ small JSON writer
Private Function J(ByVal s As String) As String
    Dim i As Long, c As Long, r As String
    For i = 1 To Len(s)
        c = AscW(Mid$(s, i, 1))
        If c < 0 Then c = c + 65536
        Select Case c
            Case 34: r = r & "\"""
            Case 92: r = r & "\\"
            Case 8: r = r & "\b"
            Case 9: r = r & "\t"
            Case 10: r = r & "\n"
            Case 12: r = r & "\f"
            Case 13: r = r & "\r"
            Case Is < 32, Is > 126: r = r & "\u" & Right$("0000" & Hex$(c), 4)
            Case Else: r = r & Chr$(c)
        End Select
    Next i
    J = """" & r & """"
End Function

Private Function JB(ByVal b As Boolean) As String
    JB = IIf(b, "true", "false")
End Function

' locale independent number: Str$ always writes "."
Private Function N(ByVal x As Double) As String
    Dim s As String
    x = Round(x, 6)
    If Abs(x) < 0.0000005 Then x = 0
    s = Trim$(Str$(x))
    If Left$(s, 1) = "." Then s = "0" & s
    If Left$(s, 2) = "-." Then s = "-0" & Mid$(s, 2)
    N = s
End Function

Private Function JPt(ByVal a As Variant) As String
    JPt = "[" & N(a(0)) & ", " & N(a(1)) & ", " & N(a(2)) & "]"
End Function

'------------------------------------------------------------------ names / paths
Private Function DocName() As String
    Dim t As String
    t = swModel.GetTitle
    If LCase$(Right$(t, 7)) = ".sldprt" Then t = Left$(t, Len(t) - 7)
    DocName = t
End Function

Private Function OutPath() As String
    Dim p As String
    p = swModel.GetPathName
    If Len(p) = 0 Then
        OutPath = Environ$("USERPROFILE") & "\" & DocName() & ".gtree.json"
    Else
        OutPath = Left$(p, InStrRev(p, ".") - 1) & ".gtree.json"
    End If
End Function
