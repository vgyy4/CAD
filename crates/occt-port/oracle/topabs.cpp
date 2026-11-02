// Deterministic reference records for OCCT 8.0.1's package TopAbs (the enumerations and TopAbs::Compose, Reverse, Complement,
// Print, the string conversions), see docs/api-topabs.md.
// Line format: `family inputs | outputs`; integers decimal. A string is printed as its length and its characters (ASCII codes);
// a result string is printed as its length and its characters as well. The enumerators are printed as their values:
// orientation 0 FORWARD, 1 REVERSED, 2 INTERNAL, 3 EXTERNAL; shape 0 COMPOUND .. 7 VERTEX, 8 SHAPE; state 0 IN, 1 OUT, 2 ON, 3 UNKNOWN.
#include <Standard_Failure.hxx>
#include <TopAbs.hxx>
#include <TopAbs_Orientation.hxx>
#include <TopAbs_ShapeEnum.hxx>
#include <TopAbs_State.hxx>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <exception>
#include <iostream>
#include <sstream>
#include <set>
#include <string>
#include <vector>

static std::string line;
static void I(long long v) { line += ' ' + std::to_string(v); }
static void B(bool v) { I(v ? 1 : 0); }
static void S(const std::string& s) {
  I((long long)s.size());
  for (unsigned char c : s) I(c);
}
static void begin(const char* name) { line = name; }
static void smash() {
  static int fill = -1;
  if (fill < 0) { const char* e = std::getenv("SMASH"); fill = e ? std::atoi(e) : 0; }
  volatile unsigned char buf[1 << 16];
  for (size_t i = 0; i < sizeof buf; ++i) buf[i] = (unsigned char)fill;
}
static void bar() {
  line += " |";
  smash();
}
static void end() { std::cout << line << std::endl; }
#define CASE(...) \
  { \
    size_t keep_ = line.size(); \
    try { \
      __VA_ARGS__ \
    } catch (Standard_Failure&) { \
      line.resize(keep_); \
      line += " throws"; \
    } catch (std::exception&) { \
      line.resize(keep_); \
      line += " throws"; \
    } \
    end(); \
  }

static uint64_t SEED = 0x9e3779b97f4a7c15ull;
static double rnd() {
  SEED ^= SEED << 13;
  SEED ^= SEED >> 7;
  SEED ^= SEED << 17;
  return (double)(SEED >> 11) / 9007199254740992.0;
}
static const char* SHAPES[9] = {"COMPOUND", "COMPSOLID", "SOLID", "SHELL", "FACE", "WIRE", "EDGE", "VERTEX", "SHAPE"};
static const char* ORIENTS[4] = {"FORWARD", "REVERSED", "INTERNAL", "EXTERNAL"};
static const char* STATES[4] = {"IN", "OUT", "ON", "UNKNOWN"};
// a string that is often a name of the table with the case changed, a prefix / a suffix added, or a random word
static std::string word(int j, const char* const* table, int n) {
  std::string s;
  int k = j % 7;
  if (k <= 3) {
    s = table[(j / 7) % n];
    if (k == 1) for (auto& c : s) c = (char)std::tolower((unsigned char)c);
    if (k == 2) for (size_t i = 0; i < s.size(); ++i) if (rnd() < 0.5) s[i] = (char)std::tolower((unsigned char)s[i]);
    if (k == 3) s += (j % 2 ? "S" : " ");
  } else if (k == 4) {
    s = table[(j / 7) % n];
    s = s.substr(0, 1 + (size_t)(rnd() * (double)s.size()));
  } else if (k == 5) {
    int len = (int)(rnd() * 9.0);
    for (int i = 0; i < len; ++i) s += (char)('A' + (int)(rnd() * 26.0));
  } else {
    int len = 1 + (int)(rnd() * 12.0);
    for (int i = 0; i < len; ++i) s += (char)('a' + (int)(rnd() * 26.0));
  }
  return s;
}

// the next word that has not been used yet in `seen`
static std::string fresh(int& j, std::set<std::string>& seen, const char* const* table, int n) {
  for (;;) {
    std::string w = word(j++, table, n);
    if (seen.insert(w).second) return w;
  }
}

int main() {
  for (int a = 0; a < 4; ++a)
    for (int b = 0; b < 4; ++b) {
      begin("compose");
      I(a); I(b);
      bar();
      CASE(I(TopAbs::Compose((TopAbs_Orientation)a, (TopAbs_Orientation)b));)
    }
  for (int a = 0; a < 4; ++a)
    for (int b = 0; b < 4; ++b)
      for (int c = 0; c < 4; ++c) {
        begin("compose3");
        I(a); I(b); I(c);
        bar();
        CASE(I(TopAbs::Compose(TopAbs::Compose((TopAbs_Orientation)a, (TopAbs_Orientation)b), (TopAbs_Orientation)c));)
      }
  for (int a = 0; a < 4; ++a) {
    begin("reverse"); I(a); bar();
    CASE(I(TopAbs::Reverse((TopAbs_Orientation)a));)
  }
  for (int a = 0; a < 4; ++a) {
    begin("complement"); I(a); bar();
    CASE(I(TopAbs::Complement((TopAbs_Orientation)a));)
  }
  for (int a = 0; a < 4; ++a) {
    begin("reverse_complement"); I(a); bar();
    CASE(I(TopAbs::Reverse(TopAbs::Complement((TopAbs_Orientation)a))); I(TopAbs::Complement(TopAbs::Reverse((TopAbs_Orientation)a)));)
  }
  for (int a = 0; a < 9; ++a) {
    begin("shape_to_string"); I(a); bar();
    CASE(S(TopAbs::ShapeTypeToString((TopAbs_ShapeEnum)a));)
  }
  for (int a = 0; a < 9; ++a) {
    begin("shape_print"); I(a); bar();
    CASE(std::ostringstream o; TopAbs::Print((TopAbs_ShapeEnum)a, o); S(o.str());)
  }
  for (int a = 0; a < 4; ++a) {
    begin("orientation_to_string"); I(a); bar();
    CASE(S(TopAbs::ShapeOrientationToString((TopAbs_Orientation)a));)
  }
  for (int a = 0; a < 4; ++a) {
    begin("orientation_print"); I(a); bar();
    CASE(std::ostringstream o; TopAbs::Print((TopAbs_Orientation)a, o); S(o.str());)
  }
  for (int a = 0; a < 4; ++a) {
    begin("state_print"); I(a); bar();
    CASE(std::ostringstream o; TopAbs::Print((TopAbs_State)a, o); S(o.str());)
  }
  std::set<std::string> seen_shape_from_string_try;
  for (int c = 0, j = 0; c < 90; ++c) {
    begin("shape_from_string_try");
    std::string s = fresh(j, seen_shape_from_string_try, SHAPES, 9);
    S(s);
    bar();
    CASE(TopAbs_ShapeEnum t = TopAbs_VERTEX; bool ok = TopAbs::ShapeTypeFromString(s.c_str(), t); B(ok); I(t);)
  }
  std::set<std::string> seen_shape_from_string;
  for (int c = 0, j = 0; c < 90; ++c) {
    begin("shape_from_string");
    std::string s = fresh(j, seen_shape_from_string, SHAPES, 9);
    S(s);
    bar();
    CASE(I(TopAbs::ShapeTypeFromString(s.c_str()));)
  }
  std::set<std::string> seen_orientation_from_string_try;
  for (int c = 0, j = 0; c < 70; ++c) {
    begin("orientation_from_string_try");
    std::string s = fresh(j, seen_orientation_from_string_try, ORIENTS, 4);
    S(s);
    bar();
    CASE(TopAbs_Orientation t = TopAbs_EXTERNAL; bool ok = TopAbs::ShapeOrientationFromString(s.c_str(), t); B(ok); I(t);)
  }
  std::set<std::string> seen_orientation_from_string;
  for (int c = 0, j = 0; c < 70; ++c) {
    begin("orientation_from_string");
    std::string s = fresh(j, seen_orientation_from_string, ORIENTS, 4);
    S(s);
    bar();
    CASE(I(TopAbs::ShapeOrientationFromString(s.c_str()));)
  }
  // the names of the states are not converted back; their printed forms are the table indexed by the enumerator
  for (int a = 0; a < 9; ++a) {
    begin("shape_roundtrip"); I(a); bar();
    CASE(TopAbs_ShapeEnum t = TopAbs_SHAPE; std::string n = TopAbs::ShapeTypeToString((TopAbs_ShapeEnum)a);
         bool ok = TopAbs::ShapeTypeFromString(n.c_str(), t); B(ok); I(t);)
  }
  for (int a = 0; a < 4; ++a) {
    begin("orientation_roundtrip"); I(a); bar();
    CASE(TopAbs_Orientation t = TopAbs_FORWARD; std::string n = TopAbs::ShapeOrientationToString((TopAbs_Orientation)a);
         bool ok = TopAbs::ShapeOrientationFromString(n.c_str(), t); B(ok); I(t);)
  }
  for (int a = 0; a < 4; ++a) {
    begin("enum_values"); I(a); bar();
    CASE(I(a == 0 ? TopAbs_FORWARD : a == 1 ? TopAbs_REVERSED : a == 2 ? TopAbs_INTERNAL : TopAbs_EXTERNAL); I(a == 0 ? TopAbs_IN : a == 1 ? TopAbs_OUT : a == 2 ? TopAbs_ON : TopAbs_UNKNOWN);
         I(a == 0 ? TopAbs_COMPOUND : a == 1 ? TopAbs_VERTEX : a == 2 ? TopAbs_SHAPE : TopAbs_FACE);)
  }
  (void)STATES;
  return 0;
}
