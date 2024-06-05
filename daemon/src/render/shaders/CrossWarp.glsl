// Author: Eke Péter <peterekepeter@gmail.com>
// License: MIT
vec4 transition(vec2 p, vec2 pr_p) {
  float x = progress;
  float pr_x = progress;
  x=smoothstep(.0,1.0,(x*2.0+p.x-1.0));
  pr_x=smoothstep(.0,1.0,(pr_x*2.0+pr_p.x-1.0));
  return mix(getFromColor((pr_p-.5)*(1.-pr_x)+.5), getToColor((p-.5)*x+.5), x);
}
