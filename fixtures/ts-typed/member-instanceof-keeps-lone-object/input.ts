class SkillManager { level = 1; }
class Player {
skill: SkillManager = new SkillManager();
make(): SkillManager { return new SkillManager(); }
}
declare const player: Player;
if (!(player.skill instanceof SkillManager)) {
const made = player.make();
}
export {};
