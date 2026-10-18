//! `/f` faction command tree — port of `factions.commands.FactionsCommandExecutor`
//! and the player subcommands.
//!
//! The original relied on a WorldEdit-style selection wand for claiming. Because the
//! port has no selection system yet, `/f claim` claims the single chunk the player
//! stands in (the classic "chunk claim" mode). This is documented in `PORTING.md`.

use std::collections::BTreeMap;

use pumpkin_plugin_api::command::{
    ArgumentType, CommandError, CommandNode, CommandSender, ConsumedArgs, StringType,
};
use pumpkin_plugin_api::commands::CommandHandler;
use pumpkin_plugin_api::{Context, Player, Server};

use super::{
    command, double_arg, int_arg, present_string_arg, register_op_permission,
    register_public_permission, reply,
};
use crate::config::Config;
use crate::utils::core;
use crate::factions::{member, Claim, ClaimManager, Faction, FactionsManager, Role};
use crate::glass::GlassType;
use crate::config::lang::Lang;
use crate::state::{state, AppState, ChunkState};
use crate::timer::{PendingAction, TimerType};
use crate::utils;

#[derive(Clone, Copy)]
enum Action {
    Create,
    Disband,
    Invite,
    Join,
    Leave,
    Kick,
    Promote,
    Demote,
    Leader,
    Rename,
    Chat,
    Claim,
    Unclaim,
    UnclaimAll,
    Map,
    Territory,
    TerritoryCoords,
    Help,
    SetHome,
    Home,
    Stuck,
    Open,
    ShowSelf,
    ShowNamed,
    List,
    ListPage,
    TopDefault,
    TopMetric,
    Deposit,
    Withdraw,
    Ally,
    Unally,
    AdminSetDtr,
    AdminAddDtr,
    AdminRemoveDtr,
    AdminRemove,
    AdminClearClaims,
    AdminSetKills,
    AdminForcePromote,
    AdminForceDemote,
    AdminForceJoin,
    AdminForceRename,
    AdminSystemCreate,
    AdminSystemList,
    AdminSystemRename,
    AdminSafezone,
    AdminEnderpearls,
    AdminAbilities,
    AdminColor,
    AdminDeathban,
    AdminClaimFor,
    AdminRemoveClaim,
    AdminChatSpy,
    AdminTpHere,
    FriendlyFire,
    Uninvite,
    AutoRevive,
    ClaimChunk,
    Rally,
    RemoveRally,
    Focus,
    Unfocus,
    Lives,
    LivesDeposit,
    LivesWithdraw,
    LivesRevive,
    AdminSetPoints,
    AdminSetBalance,
    AdminSetLives,
    AdminForceKick,
    AdminForceLeader,
    AdminFreeze,
    AdminThaw,
    AdminSave,
    HelpPage,
}

struct FactionExecutor(Action);

impl CommandHandler for FactionExecutor {
    fn handle(&self, sender: CommandSender, server: Server, args: ConsumedArgs) -> Result<i32, CommandError> {
        if let Action::Help = self.0 {
            help(&sender, None);
            return Ok(1);
        }
        if let Action::HelpPage = self.0 {
            help(&sender, present_string_arg(&args, "page"));
            return Ok(1);
        }
        run(self.0, &sender, &server, &args);
        Ok(1)
    }
}

pub fn register(context: &Context) -> Result<(), String> {
    register_public_permission(context, "lazarus:command.faction", "Use the faction commands")?;
    register_op_permission(context, "lazarus:command.faction.admin", "Use the faction admin commands")?;
    for node in [
        "lazarus:factions.dtr",
        "lazarus:factions.setpoints",
        "lazarus:factions.setbalance",
        "lazarus:factions.setlives",
        "lazarus:factions.forcekick",
        "lazarus:factions.forceleader",
        "lazarus:factions.setfreeze",
        "lazarus:factions.thaw",
        "lazarus:factions.save",
        "lazarus:factions.remove",
        "lazarus:factions.clearclaims",
        "lazarus:factions.setkills",
        "lazarus:factions.forcepromote",
        "lazarus:factions.forcedemote",
        "lazarus:factions.forcejoin",
        "lazarus:factions.forcerename",
        "lazarus:factions.systemcreate",
        "lazarus:factions.systemlist",
        "lazarus:factions.systemrename",
        "lazarus:factions.safezone",
        "lazarus:factions.enderpearls",
        "lazarus:factions.abilities",
        "lazarus:factions.color",
        "lazarus:factions.deathban",
        "lazarus:factions.claimfor",
        "lazarus:factions.removeclaim",
        "lazarus:factions.chatspy",
        "lazarus:factions.tphere",
    ] {
        register_op_permission(context, node, "Lazarus faction admin command")?;
    }

    let str_node = |name: &str| CommandNode::argument(name, &ArgumentType::String(StringType::SingleWord));
    let int_node = |name: &str| CommandNode::argument(name, &ArgumentType::Integer((None, None)));

    let root = command(&["f", "faction", "fac", "factions"], "Faction commands")
        .execute(FactionExecutor(Action::Help))
        .then(
            CommandNode::literal("help")
                .execute(FactionExecutor(Action::Help))
                .then(str_node("page").execute(FactionExecutor(Action::HelpPage))),
        )
        .then(
            CommandNode::literal("create")
                .execute(FactionExecutor(Action::Create))
                .then(str_node("name").execute(FactionExecutor(Action::Create))),
        )
        .then(
            CommandNode::literal("new")
                .execute(FactionExecutor(Action::Create))
                .then(str_node("name").execute(FactionExecutor(Action::Create))),
        )
        .then(CommandNode::literal("disband").execute(FactionExecutor(Action::Disband)))
        .then(
            CommandNode::literal("invite")
                .execute(FactionExecutor(Action::Invite))
                .then(str_node("player").execute(FactionExecutor(Action::Invite))),
        )
        .then(
            CommandNode::literal("inv")
                .execute(FactionExecutor(Action::Invite))
                .then(str_node("player").execute(FactionExecutor(Action::Invite))),
        )
        .then(
            CommandNode::literal("join")
                .execute(FactionExecutor(Action::Join))
                .then(str_node("name").execute(FactionExecutor(Action::Join))),
        )
        .then(
            CommandNode::literal("accept")
                .execute(FactionExecutor(Action::Join))
                .then(str_node("name").execute(FactionExecutor(Action::Join))),
        )
        .then(CommandNode::literal("leave").execute(FactionExecutor(Action::Leave)))
        .then(
            CommandNode::literal("kick")
                .execute(FactionExecutor(Action::Kick))
                .then(str_node("player").execute(FactionExecutor(Action::Kick))),
        )
        .then(
            CommandNode::literal("promote")
                .execute(FactionExecutor(Action::Promote))
                .then(str_node("player").execute(FactionExecutor(Action::Promote))),
        )
        .then(
            CommandNode::literal("demote")
                .execute(FactionExecutor(Action::Demote))
                .then(str_node("player").execute(FactionExecutor(Action::Demote))),
        )
        .then(
            CommandNode::literal("leader")
                .execute(FactionExecutor(Action::Leader))
                .then(str_node("player").execute(FactionExecutor(Action::Leader))),
        )
        .then(
            CommandNode::literal("rename")
                .execute(FactionExecutor(Action::Rename))
                .then(str_node("name").execute(FactionExecutor(Action::Rename))),
        )
        .then(CommandNode::literal("chat").execute(FactionExecutor(Action::Chat)))
        .then(CommandNode::literal("c").execute(FactionExecutor(Action::Chat)))
        .then(CommandNode::literal("claim").execute(FactionExecutor(Action::Claim)))
        .then(CommandNode::literal("unclaim").execute(FactionExecutor(Action::Unclaim)))
        .then(CommandNode::literal("unclaimall").execute(FactionExecutor(Action::UnclaimAll)))
        .then(CommandNode::literal("map").execute(FactionExecutor(Action::Map)))
        .then(CommandNode::literal("territory").execute(FactionExecutor(Action::Territory)).then(
            int_node("x").then(int_node("z").execute(FactionExecutor(Action::TerritoryCoords)))
        ))
        .then(CommandNode::literal("sethome").execute(FactionExecutor(Action::SetHome)))
        .then(CommandNode::literal("home").execute(FactionExecutor(Action::Home)))
        .then(CommandNode::literal("stuck").execute(FactionExecutor(Action::Stuck)))
        .then(CommandNode::literal("open").execute(FactionExecutor(Action::Open)))
        .then(
            CommandNode::literal("show")
                .execute(FactionExecutor(Action::ShowSelf))
                .then(str_node("faction").execute(FactionExecutor(Action::ShowNamed))),
        )
        .then(CommandNode::literal("i").execute(FactionExecutor(Action::ShowSelf)))
        .then(CommandNode::literal("info").execute(FactionExecutor(Action::ShowSelf)))
        .then(CommandNode::literal("who").execute(FactionExecutor(Action::ShowSelf)))
        .then(
            CommandNode::literal("list")
                .execute(FactionExecutor(Action::List))
                .then(str_node("page").execute(FactionExecutor(Action::ListPage))),
        )
        .then(
            CommandNode::literal("top")
                .execute(FactionExecutor(Action::TopDefault))
                .then(str_node("metric").execute(FactionExecutor(Action::TopMetric))),
        )
        .then(
            CommandNode::literal("deposit")
                .execute(FactionExecutor(Action::Deposit))
                .then(str_node("amount").execute(FactionExecutor(Action::Deposit))),
        )
        .then(
            CommandNode::literal("d")
                .execute(FactionExecutor(Action::Deposit))
                .then(str_node("amount").execute(FactionExecutor(Action::Deposit))),
        )
        .then(
            CommandNode::literal("withdraw")
                .execute(FactionExecutor(Action::Withdraw))
                .then(str_node("amount").execute(FactionExecutor(Action::Withdraw))),
        )
        .then(
            CommandNode::literal("w")
                .execute(FactionExecutor(Action::Withdraw))
                .then(str_node("amount").execute(FactionExecutor(Action::Withdraw))),
        )
        .then(
            CommandNode::literal("ally")
                .execute(FactionExecutor(Action::Ally))
                .then(str_node("faction").execute(FactionExecutor(Action::Ally))),
        )
        .then(
            CommandNode::literal("unally")
                .execute(FactionExecutor(Action::Unally))
                .then(str_node("faction").execute(FactionExecutor(Action::Unally))),
        )
        // ---- Admin subcommands ----
        .then(
            CommandNode::literal("setdtr")
                .execute(FactionExecutor(Action::AdminSetDtr))
                .then(
                    str_node("faction")
                        .execute(FactionExecutor(Action::AdminSetDtr))
                        .then(str_node("value").execute(FactionExecutor(Action::AdminSetDtr))),
                ),
        )
        .then(
            CommandNode::literal("adddtr")
                .execute(FactionExecutor(Action::AdminAddDtr))
                .then(
                    str_node("faction")
                        .execute(FactionExecutor(Action::AdminAddDtr))
                        .then(str_node("value").execute(FactionExecutor(Action::AdminAddDtr))),
                ),
        )
        .then(
            CommandNode::literal("removedtr")
                .execute(FactionExecutor(Action::AdminRemoveDtr))
                .then(
                    str_node("faction")
                        .execute(FactionExecutor(Action::AdminRemoveDtr))
                        .then(str_node("value").execute(FactionExecutor(Action::AdminRemoveDtr))),
                ),
        )
        .then(
            CommandNode::literal("setpoints")
                .execute(FactionExecutor(Action::AdminSetPoints))
                .then(
                    str_node("faction")
                        .execute(FactionExecutor(Action::AdminSetPoints))
                        .then(str_node("value").execute(FactionExecutor(Action::AdminSetPoints))),
                ),
        )
        .then(
            CommandNode::literal("setbalance")
                .execute(FactionExecutor(Action::AdminSetBalance))
                .then(
                    str_node("faction")
                        .execute(FactionExecutor(Action::AdminSetBalance))
                        .then(str_node("value").execute(FactionExecutor(Action::AdminSetBalance))),
                ),
        )
        .then(
            CommandNode::literal("setlives")
                .execute(FactionExecutor(Action::AdminSetLives))
                .then(
                    str_node("faction")
                        .execute(FactionExecutor(Action::AdminSetLives))
                        .then(str_node("value").execute(FactionExecutor(Action::AdminSetLives))),
                ),
        )
        .then(
            CommandNode::literal("forcekick")
                .execute(FactionExecutor(Action::AdminForceKick))
                .then(str_node("player").execute(FactionExecutor(Action::AdminForceKick))),
        )
        .then(
            CommandNode::literal("forceleader")
                .execute(FactionExecutor(Action::AdminForceLeader))
                .then(str_node("player").execute(FactionExecutor(Action::AdminForceLeader))),
        )
        .then(
            CommandNode::literal("setfreeze")
                .execute(FactionExecutor(Action::AdminFreeze))
                .then(
                    str_node("faction")
                        .execute(FactionExecutor(Action::AdminFreeze))
                        .then(str_node("time").execute(FactionExecutor(Action::AdminFreeze))),
                ),
        )
        .then(
            CommandNode::literal("setregen")
                .execute(FactionExecutor(Action::AdminFreeze))
                .then(
                    str_node("faction")
                        .execute(FactionExecutor(Action::AdminFreeze))
                        .then(str_node("time").execute(FactionExecutor(Action::AdminFreeze))),
                ),
        )
        .then(
            CommandNode::literal("thaw")
                .execute(FactionExecutor(Action::AdminThaw))
                .then(str_node("faction").execute(FactionExecutor(Action::AdminThaw))),
        )
        .then(CommandNode::literal("save").execute(FactionExecutor(Action::AdminSave)))
        .then(
            CommandNode::literal("remove")
                .execute(FactionExecutor(Action::AdminRemove))
                .then(str_node("faction").execute(FactionExecutor(Action::AdminRemove))),
        )
        .then(
            CommandNode::literal("clearclaims")
                .execute(FactionExecutor(Action::AdminClearClaims))
                .then(str_node("faction").execute(FactionExecutor(Action::AdminClearClaims))),
        )
        .then(
            CommandNode::literal("setkills")
                .execute(FactionExecutor(Action::AdminSetKills))
                .then(
                    str_node("faction")
                        .execute(FactionExecutor(Action::AdminSetKills))
                        .then(str_node("value").execute(FactionExecutor(Action::AdminSetKills))),
                ),
        )
        .then(
            CommandNode::literal("forcepromote")
                .execute(FactionExecutor(Action::AdminForcePromote))
                .then(str_node("player").execute(FactionExecutor(Action::AdminForcePromote))),
        )
        .then(
            CommandNode::literal("forcedemote")
                .execute(FactionExecutor(Action::AdminForceDemote))
                .then(str_node("player").execute(FactionExecutor(Action::AdminForceDemote))),
        )
        .then(
            CommandNode::literal("forcejoin")
                .execute(FactionExecutor(Action::AdminForceJoin))
                .then(str_node("faction").execute(FactionExecutor(Action::AdminForceJoin))),
        )
        .then(
            CommandNode::literal("forcerename")
                .execute(FactionExecutor(Action::AdminForceRename))
                .then(
                    str_node("faction")
                        .execute(FactionExecutor(Action::AdminForceRename))
                        .then(str_node("name").execute(FactionExecutor(Action::AdminForceRename))),
                ),
        )
        .then(
            CommandNode::literal("systemcreate")
                .execute(FactionExecutor(Action::AdminSystemCreate))
                .then(str_node("name").execute(FactionExecutor(Action::AdminSystemCreate))),
        )
        .then(CommandNode::literal("systemlist").execute(FactionExecutor(Action::AdminSystemList)))
        .then(
            CommandNode::literal("systemrename")
                .execute(FactionExecutor(Action::AdminSystemRename))
                .then(
                    str_node("faction")
                        .execute(FactionExecutor(Action::AdminSystemRename))
                        .then(str_node("name").execute(FactionExecutor(Action::AdminSystemRename))),
                ),
        )
        .then(
            CommandNode::literal("safezone")
                .execute(FactionExecutor(Action::AdminSafezone))
                .then(str_node("faction").execute(FactionExecutor(Action::AdminSafezone))),
        )
        .then(
            CommandNode::literal("enderpearls")
                .execute(FactionExecutor(Action::AdminEnderpearls))
                .then(str_node("faction").execute(FactionExecutor(Action::AdminEnderpearls))),
        )
        .then(
            CommandNode::literal("abilities")
                .execute(FactionExecutor(Action::AdminAbilities))
                .then(str_node("faction").execute(FactionExecutor(Action::AdminAbilities))),
        )
        .then(
            CommandNode::literal("color")
                .execute(FactionExecutor(Action::AdminColor))
                .then(
                    str_node("faction")
                        .execute(FactionExecutor(Action::AdminColor))
                        .then(str_node("color").execute(FactionExecutor(Action::AdminColor))),
                ),
        )
        .then(
            CommandNode::literal("deathban")
                .execute(FactionExecutor(Action::AdminDeathban))
                .then(str_node("faction").execute(FactionExecutor(Action::AdminDeathban))),
        )
        .then(
            CommandNode::literal("claimfor")
                .execute(FactionExecutor(Action::AdminClaimFor))
                .then(str_node("faction").execute(FactionExecutor(Action::AdminClaimFor))),
        )
        .then(CommandNode::literal("removeclaim").execute(FactionExecutor(Action::AdminRemoveClaim)))
        .then(CommandNode::literal("chatspy").execute(FactionExecutor(Action::AdminChatSpy)))
        .then(
            CommandNode::literal("tphere")
                .execute(FactionExecutor(Action::AdminTpHere))
                .then(str_node("faction").execute(FactionExecutor(Action::AdminTpHere))),
        )
        .then(CommandNode::literal("friendlyfire").execute(FactionExecutor(Action::FriendlyFire)))
        .then(CommandNode::literal("ff").execute(FactionExecutor(Action::FriendlyFire)))
        .then(
            CommandNode::literal("uninvite")
                .execute(FactionExecutor(Action::Uninvite))
                .then(str_node("player").execute(FactionExecutor(Action::Uninvite))),
        )
        .then(
            CommandNode::literal("uninv")
                .execute(FactionExecutor(Action::Uninvite))
                .then(str_node("player").execute(FactionExecutor(Action::Uninvite))),
        )
        .then(CommandNode::literal("autorevive").execute(FactionExecutor(Action::AutoRevive)))
        .then(CommandNode::literal("claimchunk").execute(FactionExecutor(Action::ClaimChunk)))
        .then(CommandNode::literal("rally").execute(FactionExecutor(Action::Rally)))
        .then(CommandNode::literal("setrally").execute(FactionExecutor(Action::Rally)))
        .then(CommandNode::literal("removerally").execute(FactionExecutor(Action::RemoveRally)))
        .then(
            CommandNode::literal("focus")
                .execute(FactionExecutor(Action::Focus))
                .then(str_node("faction").execute(FactionExecutor(Action::Focus))),
        )
        .then(CommandNode::literal("unfocus").execute(FactionExecutor(Action::Unfocus)))
        .then(
            CommandNode::literal("lives")
                .execute(FactionExecutor(Action::Lives))
                .then(str_node("faction").execute(FactionExecutor(Action::Lives))),
        )
        .then(
            CommandNode::literal("livesdeposit")
                .execute(FactionExecutor(Action::LivesDeposit))
                .then(str_node("amount").execute(FactionExecutor(Action::LivesDeposit))),
        )
        .then(
            CommandNode::literal("depositlives")
                .execute(FactionExecutor(Action::LivesDeposit))
                .then(str_node("amount").execute(FactionExecutor(Action::LivesDeposit))),
        )
        .then(
            CommandNode::literal("liveswithdraw")
                .execute(FactionExecutor(Action::LivesWithdraw))
                .then(str_node("amount").execute(FactionExecutor(Action::LivesWithdraw))),
        )
        .then(
            CommandNode::literal("withdrawlives")
                .execute(FactionExecutor(Action::LivesWithdraw))
                .then(str_node("amount").execute(FactionExecutor(Action::LivesWithdraw))),
        )
        .then(
            CommandNode::literal("livesrevive")
                .execute(FactionExecutor(Action::LivesRevive))
                .then(str_node("player").execute(FactionExecutor(Action::LivesRevive))),
        )
        .then(
            CommandNode::literal("revive")
                .execute(FactionExecutor(Action::LivesRevive))
                .then(str_node("player").execute(FactionExecutor(Action::LivesRevive))),
        );

    context.register_command(root, "lazarus:command.faction");
    Ok(())
}

fn run(action: Action, sender: &CommandSender, server: &Server, args: &ConsumedArgs) {
    // Console handling: only the original's console-capable subcommands run
    // without a player (`SubCommand#forPlayersOnly`); the rest get the same
    // `COMMANDS.FOR_PLAYERS_ONLY` message the original dispatcher sends.
    if sender.as_player().is_none() {
        if console_capable(&action) {
            run_console(action, sender, server, args);
        } else {
            for_players_only(sender);
        }
        return;
    }

    let Some(player) = sender.as_player() else {
        return;
    };
    let uuid = core::uuid_of(&player);
    let name = core::name_of(&player);

    match action {
        Action::Create => create(sender, &player, &uuid, &name, args),
        Action::Disband => disband(sender, server, &uuid),
        Action::Invite => invite(sender, server, &uuid, args),
        Action::Join => join(sender, server, &player, &uuid, &name, args),
        Action::Leave => leave(sender, server, &player, &uuid, &name),
        Action::Kick => kick(sender, server, &uuid, args),
        Action::Promote => change_role(sender, server, &uuid, args, true),
        Action::Demote => change_role(sender, server, &uuid, args, false),
        Action::Leader => transfer_leader(sender, server, &uuid, args),
        Action::Rename => rename(sender, &uuid, args),
        Action::Chat => toggle_chat(sender, &uuid),
        Action::Claim => claim(sender, &player, &uuid),
        Action::Unclaim => unclaim(sender, &player, &uuid),
        Action::UnclaimAll => unclaim_all(sender, &uuid),
        Action::Map => map(sender, &player, &uuid),
        Action::Territory => territory_info(sender, &player, &uuid, None),
        Action::TerritoryCoords => {
            let x = int_arg(args, "x").unwrap_or(0) as i32;
            let z = int_arg(args, "z").unwrap_or(0) as i32;
            territory_info(sender, &player, &uuid, Some((x, z)))
        }
        Action::Help => {}
        Action::HelpPage => {}
        Action::SetHome => set_home(sender, &player, &uuid),
        Action::Home => home(sender, &player, &uuid),
        Action::Stuck => stuck(sender, server, &player, &uuid),
        Action::Open => toggle_open(sender, server, &uuid),
        Action::ShowSelf => show(sender, server, &uuid, None),
        Action::ShowNamed => show(sender, server, &uuid, present_string_arg(args, "faction")),
        Action::List => list(sender, server, &uuid, 1),
        Action::ListPage => {
            // `StringUtils.isInteger(args[0]) ? max(1, parse) : 1`.
            let page = present_string_arg(args, "page")
                .and_then(|raw| raw.trim().parse::<i64>().ok())
                .map(|value| value.max(1) as u32)
                .unwrap_or(1);
            list(sender, server, &uuid, page);
        }
        Action::TopDefault => top_usage(sender),
        Action::TopMetric => match present_string_arg(args, "metric") {
            Some(metric) => top(sender, server, &uuid, &metric),
            None => top_usage(sender),
        },
        Action::Deposit => transfer_balance(sender, server, &uuid, args, true),
        Action::Withdraw => transfer_balance(sender, server, &uuid, args, false),
        Action::Ally => ally(sender, server, &uuid, args, true),
        Action::Unally => ally(sender, server, &uuid, args, false),
        Action::AdminSetDtr => admin_set_dtr(sender, server, args),
        Action::AdminAddDtr => admin_change_dtr(sender, server, args, true),
        Action::AdminRemoveDtr => admin_change_dtr(sender, server, args, false),
        Action::AdminRemove => admin_remove(sender, server, args),
        Action::AdminClearClaims => admin_clear_claims(sender, server, args),
        Action::AdminSetKills => admin_set_int(sender, server, args, AdminField::Kills),
        Action::AdminForcePromote => admin_force_role(sender, server, args, true),
        Action::AdminForceDemote => admin_force_role(sender, server, args, false),
        Action::AdminForceJoin => admin_force_join(sender, server, args),
        Action::AdminForceRename => admin_force_rename(sender, server, args),
        Action::AdminSystemCreate => admin_system_create(sender, server, args),
        Action::AdminSystemList => admin_system_list(sender),
        Action::AdminSystemRename => admin_system_rename(sender, server, args),
        Action::AdminSafezone => admin_toggle_system_flag(sender, server, args, SystemFlag::Safezone),
        Action::AdminEnderpearls => admin_toggle_system_flag(sender, server, args, SystemFlag::Enderpearls),
        Action::AdminAbilities => admin_toggle_system_flag(sender, server, args, SystemFlag::Abilities),
        Action::AdminColor => admin_color(sender, server, args),
        Action::AdminDeathban => admin_deathban(sender, server, args),
        Action::AdminClaimFor => admin_claim_for(sender, server, args),
        Action::AdminRemoveClaim => admin_remove_claim(sender, server),
        Action::AdminChatSpy => admin_chat_spy(sender, server),
        Action::AdminTpHere => admin_tp_here(sender, server, args),
        Action::FriendlyFire => friendly_fire(sender, server),
        Action::Uninvite => uninvite(sender, server, args),
        Action::AutoRevive => auto_revive(sender, server),
        Action::ClaimChunk => claim_chunk(sender, server, &player, &uuid),
        Action::Rally => rally(sender, server, &player, &uuid),
        Action::RemoveRally => remove_rally(sender, server, &player, &uuid),
        Action::Focus => focus(sender, server, &uuid, args),
        Action::Unfocus => unfocus(sender, server, &uuid),
        Action::Lives => lives(sender, args),
        Action::LivesDeposit => lives_transfer(sender, server, &uuid, args, true),
        Action::LivesWithdraw => lives_transfer(sender, server, &uuid, args, false),
        Action::LivesRevive => lives_revive(sender, server, &uuid, args),
        Action::AdminSetPoints => admin_set_int(sender, server, args, AdminField::Points),
        Action::AdminSetBalance => admin_set_int(sender, server, args, AdminField::Balance),
        Action::AdminSetLives => admin_set_int(sender, server, args, AdminField::Lives),
        Action::AdminForceKick => admin_force_kick(sender, server, args),
        Action::AdminForceLeader => admin_force_leader(sender, server, args),
        Action::AdminFreeze => admin_set_freeze(sender, server, args),
        Action::AdminThaw => admin_thaw(sender, server, args),
        Action::AdminSave => admin_save(sender, server),
    }
}

// ---------------------------------------------------------------------------
// Individual subcommands
// ---------------------------------------------------------------------------

/// `SubCommand#forPlayersOnly` counterpart: whether console may run `action`.
fn console_capable(action: &Action) -> bool {
    matches!(
        action,
        Action::ShowNamed
            | Action::List
            | Action::ListPage
            | Action::TopDefault
            | Action::TopMetric
            | Action::Lives
            | Action::AdminSetDtr
            | Action::AdminAddDtr
            | Action::AdminRemoveDtr
            | Action::AdminRemove
            | Action::AdminClearClaims
            | Action::AdminSetKills
            | Action::AdminForcePromote
            | Action::AdminForceDemote
            | Action::AdminForceRename
            | Action::AdminSystemCreate
            | Action::AdminSystemList
            | Action::AdminSystemRename
            | Action::AdminSafezone
            | Action::AdminEnderpearls
            | Action::AdminAbilities
            | Action::AdminColor
            | Action::AdminDeathban
            | Action::AdminSetPoints
            | Action::AdminSetBalance
            | Action::AdminSetLives
            | Action::AdminForceKick
            | Action::AdminForceLeader
            | Action::AdminFreeze
            | Action::AdminThaw
            | Action::AdminSave
    )
}

/// `FACTION_PREFIX + COMMANDS.FOR_PLAYERS_ONLY` (the dispatcher message).
fn for_players_only(sender: &CommandSender) {
    let (prefix, template) = {
        let guard = state().read().unwrap();
        (
            guard.raw.language.string("FACTION_PREFIX"),
            guard.raw.language.string("COMMANDS.FOR_PLAYERS_ONLY"),
        )
    };
    reply(sender, format!("{prefix}{template}"));
}

/// Dispatches the console-capable subcommands (no player context available).
fn run_console(action: Action, sender: &CommandSender, server: &Server, args: &ConsumedArgs) {
    match action {
        Action::ShowNamed => show(sender, server, "", present_string_arg(args, "faction")),
        Action::List => list(sender, server, "", 1),
        Action::ListPage => {
            // `StringUtils.isInteger(args[0]) ? max(1, parse) : 1`.
            let page = present_string_arg(args, "page")
                .and_then(|raw| raw.trim().parse::<i64>().ok())
                .map(|value| value.max(1) as u32)
                .unwrap_or(1);
            list(sender, server, "", page);
        }
        Action::TopDefault => top_usage(sender),
        Action::TopMetric => match present_string_arg(args, "metric") {
            Some(metric) => top(sender, server, "", &metric),
            None => top_usage(sender),
        },
        Action::Lives => lives(sender, args),
        Action::AdminSetDtr => admin_set_dtr(sender, server, args),
        Action::AdminAddDtr => admin_change_dtr(sender, server, args, true),
        Action::AdminRemoveDtr => admin_change_dtr(sender, server, args, false),
        Action::AdminRemove => admin_remove(sender, server, args),
        Action::AdminClearClaims => admin_clear_claims(sender, server, args),
        Action::AdminSetKills => admin_set_int(sender, server, args, AdminField::Kills),
        Action::AdminForcePromote => admin_force_role(sender, server, args, true),
        Action::AdminForceDemote => admin_force_role(sender, server, args, false),
        Action::AdminForceRename => admin_force_rename(sender, server, args),
        Action::AdminSystemCreate => admin_system_create(sender, server, args),
        Action::AdminSystemList => admin_system_list(sender),
        Action::AdminSystemRename => admin_system_rename(sender, server, args),
        Action::AdminSafezone => admin_toggle_system_flag(sender, server, args, SystemFlag::Safezone),
        Action::AdminEnderpearls => {
            admin_toggle_system_flag(sender, server, args, SystemFlag::Enderpearls)
        }
        Action::AdminAbilities => admin_toggle_system_flag(sender, server, args, SystemFlag::Abilities),
        Action::AdminColor => admin_color(sender, server, args),
        Action::AdminDeathban => admin_deathban(sender, server, args),
        Action::AdminSetPoints => admin_set_int(sender, server, args, AdminField::Points),
        Action::AdminSetBalance => admin_set_int(sender, server, args, AdminField::Balance),
        Action::AdminSetLives => admin_set_int(sender, server, args, AdminField::Lives),
        Action::AdminForceKick => admin_force_kick(sender, server, args),
        Action::AdminForceLeader => admin_force_leader(sender, server, args),
        Action::AdminFreeze => admin_set_freeze(sender, server, args),
        Action::AdminThaw => admin_thaw(sender, server, args),
        Action::AdminSave => admin_save(sender, server),
        _ => {}
    }
}

fn create(sender: &CommandSender, _player: &Player, uuid: &str, name: &str, args: &ConsumedArgs) {
    let Some(faction_name) = present_string_arg(args, "name") else {
        reply(sender, &state().read().unwrap().lang.usage_create);
        return;
    };

    let messages;
    {
        let mut guard = state().write().unwrap();
        let config = guard.config.clone();
        let lang = guard.lang.clone();

        if guard.factions.by_member(uuid).is_some() {
            reply(sender, &lang.faction_already_in_self);
            return;
        }

        if faction_name.chars().count() < config.factions.name_min_length {
            reply(sender, utils::replace(&lang.faction_name_too_short, "<min>", &config.factions.name_min_length.to_string()));
            return;
        }
        if faction_name.chars().count() > config.factions.name_max_length {
            reply(sender, utils::replace(&lang.faction_name_too_long, "<max>", &config.factions.name_max_length.to_string()));
            return;
        }
        if config.factions.disallowed_names.iter().any(|n| n.eq_ignore_ascii_case(&faction_name)) {
            reply(sender, &lang.faction_blocked_name);
            return;
        }
        if guard.factions.name_taken(&faction_name) {
            reply(sender, utils::replace(&lang.faction_already_exists, "<name>", &faction_name));
            return;
        }

        let id = guard.factions.create(faction_name.clone(), &config);
        guard.factions.add_member(&id, member(uuid.to_string(), name.to_string(), Role::Leader));

        messages = utils::replace(&lang.faction_created, "<name>", &faction_name)
            .replace("<player>", name);
    }

    reply(sender, messages);
}

fn disband(sender: &CommandSender, server: &Server, uuid: &str) {
    let messages: Option<String>;
    {
        let mut guard = state().write().unwrap();
        let lang = guard.lang.clone();
        let config = guard.config.clone();

        let Some(faction_id) = guard.factions.by_member(uuid).map(|f| f.id.clone()) else {
            reply(sender, &lang.faction_not_in);
            return;
        };
        let faction = guard.factions.get(&faction_id).cloned().unwrap();

        if !is_leader(&faction, uuid) {
            reply(sender, &lang.faction_leader_only);
            return;
        }
        if config.factions.disband_while_frozen && guard.timers.is_frozen(&faction_id) {
            reply(sender, &lang.faction_cannot_disband_frozen);
            return;
        }

        guard.claims.remove_all(&faction_id);
        guard.factions.disband(&faction_id);

        let message = utils::replace(
            &utils::replace(&lang.faction_disbanded, "<name>", &faction.name),
            "<player>",
            &sender.get_name(),
        );
        // `PlayerFaction#sendMessage` reaches every member, the commanding leader
        // included, so there is no separate reply here (that caused a double send).
        for member in faction.members.values() {
            if let Some(uuid) = pumpkin_plugin_api::uuid::parse(&member.uuid)
                && let Some(player) = server.get_player_by_uuid(uuid)
            {
                core::send(&player, &message);
            }
        }
        messages = None;
    }
    if let Some(message) = messages {
        reply(sender, message);
    }
}

fn invite(sender: &CommandSender, server: &Server, uuid: &str, args: &ConsumedArgs) {
    let Some(target_name) = present_string_arg(args, "player") else {
        reply(sender, &state().read().unwrap().lang.usage_invite);
        return;
    };

    let messages;
    {
        let mut guard = state().write().unwrap();
        let lang = guard.lang.clone();
        let config = guard.config.clone();

        let Some(faction_id) = guard.factions.by_member(uuid).map(|f| f.id.clone()) else {
            reply(sender, &lang.faction_not_in);
            return;
        };

        let Some(target) = server.get_player_by_name(&target_name) else {
            reply(sender, utils::replace(&lang.player_not_online, "<player>", &target_name));
            return;
        };
        let target_uuid = core::uuid_of(&target);

        if guard.factions.by_member(&target_uuid).is_some() {
            reply(sender, utils::replace(&lang.faction_already_in_others, "<player>", &target_name));
            return;
        }

        let Some(faction) = guard.factions.get(&faction_id) else {
            return;
        };

        if faction.invitations.contains(&target_uuid) {
            reply(sender, utils::replace(&lang.faction_invite_already, "<player>", &target_name));
            return;
        }
        if faction.members.len() >= config.factions.faction_limit {
            reply(sender, &lang.faction_full);
            return;
        }

        let faction_name = faction.name.clone();
        if let Some(faction) = guard.factions.get_mut(&faction_id) {
            faction.invitations.push(target_uuid.clone());
        }

        let to_target = utils::replace(&lang.faction_invited, "<name>", &faction_name);
        let to_sender = utils::replace(&lang.faction_invite_others, "<player>", &target_name);

        core::send(&target, &to_target);
        messages = to_sender;
    }
    reply(sender, messages);
}

fn join(sender: &CommandSender, server: &Server, player: &Player, uuid: &str, name: &str, args: &ConsumedArgs) {
    let Some(faction_name) = present_string_arg(args, "name") else {
        reply(sender, &state().read().unwrap().lang.usage_join);
        return;
    };

    let messages;
    {
        let mut guard = state().write().unwrap();
        let lang = guard.lang.clone();
        let config = guard.config.clone();

        if guard.factions.by_member(uuid).is_some() {
            reply(sender, &lang.faction_already_in_self);
            return;
        }

        let Some(faction_id) = search_player_faction(&guard, &faction_name) else {
            reply(sender, utils::replace(&lang.faction_doesnt_exist, "<argument>", &faction_name));
            return;
        };

        let invited = guard.factions.get(&faction_id).is_some_and(|f| f.invitations.contains(&uuid.to_string()));
        let open = guard.factions.get(&faction_id).is_some_and(|f| f.open);
        if !invited && !open {
            reply(sender, &lang.faction_no_invitation);
            return;
        }

        if config.factions.join_while_frozen && guard.timers.is_frozen(&faction_id) {
            reply(sender, &lang.faction_cannot_join_frozen);
            return;
        }

        if let Some(faction) = guard.factions.get(&faction_id) {
            if faction.members.len() >= config.factions.faction_limit {
                reply(sender, &lang.faction_full);
                return;
            }
        }

        guard.factions.add_member(&faction_id, member(uuid.to_string(), name.to_string(), Role::Member));
        if let Some(faction) = guard.factions.get_mut(&faction_id) {
            faction.invitations.retain(|id| id != uuid);
        }
        guard
            .chunk_cache
            .insert(uuid.to_string(), ChunkState::default());

        let faction = guard.factions.get(&faction_id).cloned().unwrap();
        let broadcast = utils::replace(&lang.faction_member_joined, "<player>", name);
        for member in faction.members.values() {
            if member.uuid == uuid {
                continue;
            }
            if let Some(id) = pumpkin_plugin_api::uuid::parse(&member.uuid)
                && let Some(online) = server.get_player_by_uuid(id)
            {
                core::send(&online, &broadcast);
            }
        }
        messages = utils::replace(&lang.faction_joined, "<player>", name);
        let _ = player;
    }
    reply(sender, messages);
}

fn leave(sender: &CommandSender, server: &Server, player: &Player, uuid: &str, name: &str) {
    let messages;
    {
        let mut guard = state().write().unwrap();
        let lang = guard.lang.clone();
        let config = guard.config.clone();

        let Some(faction_id) = guard.factions.by_member(uuid).map(|f| f.id.clone()) else {
            reply(sender, &lang.faction_not_in);
            return;
        };
        let faction = guard.factions.get(&faction_id).cloned().unwrap();

        if is_leader(&faction, uuid) && faction.members.len() > 1 {
            reply(sender, &lang.faction_leader_only);
            return;
        }
        if !config.factions.leave_while_frozen && guard.timers.is_frozen(&faction_id) {
            reply(sender, &lang.faction_cannot_leave_frozen);
            return;
        }

        guard.factions.remove_member(&faction_id, uuid);
        if faction.members.len() <= 1 {
            guard.claims.remove_all(&faction_id);
            guard.factions.disband(&faction_id);
        }

        let broadcast = utils::replace(&lang.faction_member_left, "<player>", name);
        for member in faction.members.values() {
            if member.uuid == uuid {
                continue;
            }
            if let Some(id) = pumpkin_plugin_api::uuid::parse(&member.uuid)
                && let Some(online) = server.get_player_by_uuid(id)
            {
                core::send(&online, &broadcast);
            }
        }
        messages = utils::replace(&lang.faction_left, "<name>", &faction.name);
        let _ = player;
    }
    reply(sender, messages);
}

fn kick(sender: &CommandSender, server: &Server, uuid: &str, args: &ConsumedArgs) {
    let Some(target_name) = present_string_arg(args, "player") else {
        reply(sender, &state().read().unwrap().lang.usage_kick);
        return;
    };

    let messages;
    {
        let mut guard = state().write().unwrap();
        let lang = guard.lang.clone();

        let Some(faction_id) = guard.factions.by_member(uuid).map(|f| f.id.clone()) else {
            reply(sender, &lang.faction_not_in);
            return;
        };
        let faction = guard.factions.get(&faction_id).cloned().unwrap();

        let Some(sender_role) = faction.member(uuid).map(|m| m.role) else {
            return;
        };
        if !sender_role.is_at_least(Role::Captain) {
            reply(sender, &lang.faction_leader_only);
            return;
        }

        let Some(target) = server.get_player_by_name(&target_name) else {
            reply(sender, utils::replace(&lang.player_not_online, "<player>", &target_name));
            return;
        };
        let target_uuid = core::uuid_of(&target);

        let Some(target_role) = faction.member(&target_uuid).map(|m| m.role) else {
            reply(sender, utils::replace(&lang.faction_not_in_others, "<player>", &target_name));
            return;
        };
        if target_role.is_at_least(sender_role) {
            reply(sender, &lang.faction_leader_only);
            return;
        }
        if target_uuid == uuid {
            return;
        }

        guard.factions.remove_member(&faction_id, &target_uuid);
        core::send(&target, &utils::replace(&lang.faction_you_were_kicked, "<name>", &faction.name));

        messages = utils::replace(&lang.faction_kicked, "<player>", &target_name);
    }
    reply(sender, messages);
}

fn change_role(sender: &CommandSender, server: &Server, uuid: &str, args: &ConsumedArgs, promote: bool) {
    let Some(target_name) = present_string_arg(args, "player") else {
        let usage = if promote {
            state().read().unwrap().lang.usage_promote.clone()
        } else {
            state().read().unwrap().lang.usage_demote.clone()
        };
        reply(sender, usage);
        return;
    };

    let messages;
    {
        let mut guard = state().write().unwrap();
        let lang = guard.lang.clone();

        let Some(faction_id) = guard.factions.by_member(uuid).map(|f| f.id.clone()) else {
            reply(sender, &lang.faction_not_in);
            return;
        };
        let faction = guard.factions.get(&faction_id).cloned().unwrap();

        let Some(sender_role) = faction.member(uuid).map(|m| m.role) else {
            return;
        };
        if !sender_role.is_at_least(Role::CoLeader) {
            reply(sender, &lang.faction_leader_only);
            return;
        }

        let Some(target) = server.get_player_by_name(&target_name) else {
            reply(sender, utils::replace(&lang.player_not_online, "<player>", &target_name));
            return;
        };
        let target_uuid = core::uuid_of(&target);

        let Some(target_role) = faction.member(&target_uuid).map(|m| m.role) else {
            reply(sender, utils::replace(&lang.faction_not_in_others, "<player>", &target_name));
            return;
        };

        let new_role = if promote { target_role.promote() } else { target_role.demote() };
        let Some(new_role) = new_role else {
            return;
        };

        if let Some(faction) = guard.factions.get_mut(&faction_id)
            && let Some(member) = faction.member_mut(&target_uuid)
        {
            member.role = new_role;
        }

        let template = if promote { &lang.faction_promoted } else { &lang.faction_demoted };
        messages = utils::replace(
            &utils::replace(template, "<player>", &target_name),
            "<role>",
            new_role.name(),
        );
    }
    reply(sender, messages);
}

fn transfer_leader(sender: &CommandSender, server: &Server, uuid: &str, args: &ConsumedArgs) {
    let Some(target_name) = present_string_arg(args, "player") else {
        reply(sender, &state().read().unwrap().lang.usage_leader);
        return;
    };

    let messages;
    {
        let mut guard = state().write().unwrap();
        let lang = guard.lang.clone();

        let Some(faction_id) = guard.factions.by_member(uuid).map(|f| f.id.clone()) else {
            reply(sender, &lang.faction_not_in);
            return;
        };
        let faction = guard.factions.get(&faction_id).cloned().unwrap();
        if !is_leader(&faction, uuid) {
            reply(sender, &lang.faction_leader_only);
            return;
        }

        let Some(target) = server.get_player_by_name(&target_name) else {
            reply(sender, utils::replace(&lang.player_not_online, "<player>", &target_name));
            return;
        };
        let target_uuid = core::uuid_of(&target);
        if !faction.members.contains_key(&target_uuid) {
            reply(sender, utils::replace(&lang.faction_not_in_others, "<player>", &target_name));
            return;
        }

        if let Some(faction) = guard.factions.get_mut(&faction_id) {
            if let Some(old_leader) = faction.member_mut(uuid) {
                old_leader.role = Role::CoLeader;
            }
            if let Some(new_leader) = faction.member_mut(&target_uuid) {
                new_leader.role = Role::Leader;
            }
        }

        messages = utils::replace(
            &utils::replace(&lang.faction_leader_changed, "<player>", &sender.get_name()),
            "<target>",
            &target_name,
        );
    }
    reply(sender, messages);
}

fn rename(sender: &CommandSender, uuid: &str, args: &ConsumedArgs) {
    let Some(new_name) = present_string_arg(args, "name") else {
        reply(sender, &state().read().unwrap().lang.usage_rename);
        return;
    };

    let messages;
    {
        let mut guard = state().write().unwrap();
        let lang = guard.lang.clone();
        let config = guard.config.clone();

        let Some(faction_id) = guard.factions.by_member(uuid).map(|f| f.id.clone()) else {
            reply(sender, &lang.faction_not_in);
            return;
        };
        let faction = guard.factions.get(&faction_id).cloned().unwrap();
        if !is_leader(&faction, uuid) {
            reply(sender, &lang.faction_leader_only);
            return;
        }
        if guard.factions.name_taken(&new_name) {
            reply(sender, utils::replace(&lang.faction_already_exists, "<name>", &new_name));
            return;
        }
        if new_name.chars().count() < config.factions.name_min_length
            || new_name.chars().count() > config.factions.name_max_length
        {
            reply(sender, &lang.faction_blocked_name);
            return;
        }
        if let Some(faction) = guard.factions.get_mut(&faction_id) {
            faction.name = new_name.clone();
        }
        messages = utils::replace(
            &utils::replace(&lang.faction_renamed, "<name>", &faction.name),
            "<newName>",
            &new_name,
        );
    }
    reply(sender, messages);
}

fn toggle_chat(sender: &CommandSender, uuid: &str) {
    let mut guard = state().write().unwrap();
    let lang = guard.lang.clone();
    let Some(faction_id) = guard.factions.by_member(uuid).map(|f| f.id.clone()) else {
        reply(sender, &lang.faction_not_in);
        return;
    };
    let mut new_type = crate::factions::ChatType::Public;
    if let Some(faction) = guard.factions.get_mut(&faction_id)
        && let Some(member) = faction.member_mut(uuid)
    {
        member.chat_type = match member.chat_type {
            crate::factions::ChatType::Public => crate::factions::ChatType::Faction,
            crate::factions::ChatType::Faction => crate::factions::ChatType::Ally,
            crate::factions::ChatType::Ally => crate::factions::ChatType::Public,
        };
        new_type = member.chat_type;
    }
    let status = match new_type {
        crate::factions::ChatType::Public => "public",
        crate::factions::ChatType::Faction => "faction",
        crate::factions::ChatType::Ally => "ally",
    };
    reply(sender, utils::replace(&lang.faction_chat_toggled, "<status>", status));
}

fn claim(sender: &CommandSender, player: &Player, uuid: &str) {
    let error = {
        let guard = state().read().unwrap();
        validate_claim(player, uuid, &guard.factions, &guard.claims, &guard.config, &guard.lang)
    };

    if let Err(message) = error {
        reply(sender, message);
        return;
    }

    // The original always (re)starts the selection process, it never toggles off.
    state().write().unwrap().selection.start(player);
    let message = state().read().unwrap().lang.faction_claim_wand_received.clone();
    reply(sender, message);
}

fn validate_claim(
    player: &Player,
    uuid: &str,
    factions: &FactionsManager,
    claims: &ClaimManager,
    config: &Config,
    lang: &Lang,
) -> Result<(), String> {
    let Some(faction_id) = factions.by_member(uuid).map(|f| f.id.clone()) else {
        return Err(lang.faction_not_in.clone());
    };
    let faction = factions.get(&faction_id).unwrap();

    if faction
        .member(uuid)
        .map(|m| m.role)
        .is_none_or(|role| !role.is_at_least(Role::CoLeader))
    {
        return Err(lang.faction_leader_only.clone());
    }

    let max_claims = config
        .factions
        .max_claims
        .min(config.factions.claims_per_player * faction.members.len());
    if claims.claim_count(&faction_id) >= max_claims {
        return Err(lang.faction_claim_limit.clone());
    }

    if core::dimension_of(&core::world_name(player)) != core::Dimension::Overworld {
        return Err(lang.faction_claim_only_overworld.clone());
    }

    let location = core::location(player);
    let warzone = config.warzone_radius.overworld;
    if location.block_x().abs() <= warzone && location.block_z().abs() <= warzone {
        return Err(utils::replace(
            &lang.faction_claim_spawn_too_close,
            "<amount>",
            &warzone.to_string(),
        ));
    }

    if (0..36u8).all(|slot| player.get_inventory_item(slot).is_some()) {
        return Err(lang.faction_claim_inventory_full.clone());
    }

    Ok(())
}

fn unclaim(sender: &CommandSender, player: &Player, uuid: &str) {
    let loc = core::location(player);
    let messages;
    {
        let mut guard = state().write().unwrap();
        let lang = guard.lang.clone();

        let Some(faction_id) = guard.factions.by_member(uuid).map(|f| f.id.clone()) else {
            reply(sender, &lang.faction_not_in);
            return;
        };
        let owned = guard
            .claims
            .faction_at(&loc.world, loc.block_x(), loc.block_z())
            .map(|owner| owner == &faction_id)
            .unwrap_or(false);
        if !owned {
            reply(sender, &lang.faction_claim_not_owned);
            return;
        }
        guard
            .claims
            .remove_claim_at(&faction_id, &loc.world, loc.block_x(), loc.block_z());
        messages = utils::replace(
            &utils::replace(&lang.faction_claim_removed, "<player>", &sender.get_name()),
            "<location>",
            &format!("{}, {}", loc.block_x(), loc.block_z()),
        );
    }
    reply(sender, messages);
}

fn unclaim_all(sender: &CommandSender, uuid: &str) {
    let messages;
    {
        let mut guard = state().write().unwrap();
        let lang = guard.lang.clone();

        let Some(faction_id) = guard.factions.by_member(uuid).map(|f| f.id.clone()) else {
            reply(sender, &lang.faction_not_in);
            return;
        };
        let faction = guard.factions.get(&faction_id).cloned().unwrap();
        if !is_leader(&faction, uuid) {
            reply(sender, &lang.faction_leader_only);
            return;
        }
        guard.claims.remove_all(&faction_id);
        messages = "&aAll faction claims removed.".to_string();
    }
    reply(sender, messages);
}

fn map(sender: &CommandSender, player: &Player, uuid: &str) {
    let world = core::world_name(player);
    let message;
    {
        let mut guard = state().write().unwrap();
        let AppState { glass, claims, config, .. } = &mut *guard;

        if glass.is_map_user(uuid) {
            glass.clear_type(player, GlassType::ClaimMap);
            glass.remove_map_user(uuid);
            message = "&eAll claim pillars are now &chidden&e.".to_string();
        } else {
            // Force a regeneration on the next map update.
            glass.set_map_user(uuid, &world, i32::MIN, i32::MIN);
            crate::glass::update_map(glass, player, claims, config);
            message = if glass.is_map_user(uuid) {
                "&aClaim map enabled.".to_string()
            } else {
                "&cThere are no claims nearby.".to_string()
            };
        }
    }
    reply(sender, message);
}

/// Diagnostic helper (not part of the original): prints the territory square with
/// the warzone/safezone bounds centered at 0,0.
fn territory_info(
    sender: &CommandSender,
    player: &Player,
    uuid: &str,
    coords: Option<(i32, i32)>,
) {
    let location = core::location(player);
    let world = location.world.clone();
    let (bx, bz) = coords.unwrap_or((location.block_x(), location.block_z()));

    let war_radius = {
        let guard = state().read().unwrap();
        match core::dimension_of(&world) {
            core::Dimension::Overworld => guard.config.warzone_radius.overworld,
            core::Dimension::Nether => guard.config.warzone_radius.nether,
            core::Dimension::End => guard.config.warzone_radius.end,
        }
    };

    let r = war_radius;
    let dist = bx.abs().max(bz.abs());

    let territory = core::territory_at(&world, bx, bz);
    let viewer = core::faction_id_of(uuid);
    let display = core::territory_display(&territory, viewer.as_deref());

    let reason = if matches!(territory, core::Territory::Safezone) {
        "Safezone (claimed by the spawn faction)"
    } else if dist <= r {
        "Warzone (dist <= warzone)"
    } else {
        "Wilderness (dist > warzone)"
    };

    let sep = format!("&7&m{}", " ".repeat(80));
    reply(sender, &sep);
    reply(sender, "&6&lWarzone square (centro 0,0)");
    reply(sender, format!(" &eWorld: &f{world}"));
    reply(sender, format!(" &eWarzone: &fX[-{r}, {r}] &7Z[&f-{r}, {r}&7] &8(|x|,|z| <= {r})"));
    reply(
        sender,
        format!(
            " &eCorners: &f(-{r},-{r}) (-{r},{r}) ({r},-{r}) ({r},{r})"
        ),
    );
    reply(sender, format!(" &eLado: &f{} &ebloques por eje (inclusivo)", r * 2 + 1));
    reply(sender, " &7Protección &finfinita en Y&7 (arriba/abajo).");
    reply(sender, &sep);

    reply(sender, format!(" &eQuery: &f{bx}, {bz}"));
    reply(sender, format!(" &eDist: &fmax(|x|,|z|) = {dist}"));
    reply(sender, format!(" &eCurrent: {display}"));
    reply(sender, format!(" &eReason: &f{reason}"));
    reply(sender, &sep);

    // Corner verification: every corner must be Warzone, and one block outside must
    // be Wilderness — proves the square on all four sides.
    reply(sender, "&6&lEsquinas del cuadrado (deben ser Warzone):");
    for (cx, cz) in [(-r, -r), (-r, r), (r, -r), (r, r)] {
        let t = core::territory_at(&world, cx, cz);
        let ok = matches!(t, core::Territory::Warzone);
        let viewer = core::faction_id_of(uuid);
        let d = core::territory_display(&t, viewer.as_deref());
        reply(sender, format!(" &e({cx}, {cz}) -> {d}  {}", if ok { "&aOK" } else { "&cFALLO" }));
    }

    reply(sender, "&6&l1 bloque fuera (deben ser Wilderness):");
    for (cx, cz) in [(r + 1, 0), (-(r + 1), 0), (0, r + 1), (0, -(r + 1))] {
        let t = core::territory_at(&world, cx, cz);
        let ok = matches!(t, core::Territory::Wilderness);
        let viewer = core::faction_id_of(uuid);
        let d = core::territory_display(&t, viewer.as_deref());
        reply(sender, format!(" &e({cx}, {cz}) -> {d}  {}", if ok { "&aOK" } else { "&cFALLO" }));
    }
    reply(sender, &sep);
}

fn set_home(sender: &CommandSender, player: &Player, uuid: &str) {
    let messages;
    {
        let mut guard = state().write().unwrap();
        let lang = guard.lang.clone();

        let Some(faction_id) = guard.factions.by_member(uuid).map(|f| f.id.clone()) else {
            reply(sender, &lang.faction_not_in);
            return;
        };
        let faction = guard.factions.get(&faction_id).cloned().unwrap();
        if faction.member(uuid).map(|m| m.role).is_none_or(|role| !role.is_at_least(Role::CoLeader)) {
            reply(sender, &lang.faction_leader_only);
            return;
        }
        let home = core::location(player);
        if let Some(faction) = guard.factions.get_mut(&faction_id) {
            faction.home = Some(home);
        }
        messages = lang.faction_home_set.clone();
    }
    reply(sender, messages);
}

fn home(sender: &CommandSender, player: &Player, uuid: &str) {
    let messages;
    {
        let mut guard = state().write().unwrap();
        let lang = guard.lang.clone();

        let Some(faction_id) = guard.factions.by_member(uuid).map(|f| f.id.clone()) else {
            reply(sender, &lang.faction_not_in);
            return;
        };
        let Some(home) = guard.factions.get(&faction_id).and_then(|f| f.home.clone()) else {
            reply(sender, &lang.faction_home_not_set);
            return;
        };

        // PvP protection denies `/f home` unless own claims may be entered
        // (port of `HomeCommand#execute`).
        if !guard.raw.config.bool("PVP_PROTECTION.CAN_ENTER_OWN_CLAIM")
            && guard.timers.is_active(uuid, TimerType::PvpProtection)
        {
            let prefix = guard.raw.language.string("FACTION_PREFIX");
            let message = guard
                .raw
                .language
                .string("FACTIONS.HOME_COMMAND.CANNOT_TELEPORT.WITH_PVP_TIMER");
            reply(sender, &format!("{prefix}{message}"));
            return;
        }

        let warmup = match core::dimension_of(&home.world) {
            core::Dimension::Overworld => guard.config.home_warmup.overworld,
            core::Dimension::Nether => guard.config.home_warmup.nether,
            core::Dimension::End => guard.config.home_warmup.end,
        };

        let origin = core::location(player);
        let expiry = crate::utils::now_millis() + (warmup as i64 * 1000);
        guard.timers.set_pending(
            uuid,
            PendingAction {
                timer_type: TimerType::Home,
                expiry,
                origin_world: origin.world,
                origin_x: origin.x,
                origin_y: origin.y,
                origin_z: origin.z,
                target: Some(home),
            },
        );

        messages = utils::replace(&lang.faction_home_teleport, "<seconds>", &warmup.to_string());
    }
    reply(sender, messages);
}

fn stuck(sender: &CommandSender, server: &Server, player: &Player, uuid: &str) {
    let messages;
    {
        let mut guard = state().write().unwrap();
        let prefix = guard.raw.language.string("FACTION_PREFIX");
        let origin = core::location(player);

        if core::dimension_of(&origin.world) != core::Dimension::Overworld {
            let msg = guard
                .raw
                .language
                .string("FACTIONS.STUCK_COMMAND.CANNOT_TELEPORT.ONLY_IN_OVERWORLD");
            drop(guard);
            return reply(sender, format!("{prefix}{msg}"));
        }

        let territory =
            core::territory_at_guard(&guard, &origin.world, origin.block_x(), origin.block_z());
        let own = guard.factions.by_member(uuid).map(|f| f.id.clone());

        let allowed = match &territory {
            // Only enemy *player* claims (the user wants no warzone/wilderness).
            core::Territory::Faction(id) => own.as_deref() != Some(id.as_str()),
            _ => false,
        };

        if !allowed {
            let key = match &territory {
                core::Territory::Faction(id) if own.as_deref() == Some(id.as_str()) => {
                    "FACTIONS.STUCK_COMMAND.CANNOT_TELEPORT.IN_OWN_CLAIM"
                }
                _ => "FACTIONS.STUCK_COMMAND.CANNOT_TELEPORT.ONLY_FROM_ENEMY_CLAIMS",
            };
            let msg = guard.raw.language.string(key);
            drop(guard);
            return reply(sender, format!("{prefix}{msg}"));
        }

        if guard.timers.pending(uuid).map(|p| p.timer_type) == Some(TimerType::Stuck) {
            let msg = guard
                .raw
                .language
                .string("FACTIONS.STUCK_COMMAND.CANNOT_TELEPORT.ALREADY_TELEPORTING");
            drop(guard);
            return reply(sender, format!("{prefix}{msg}"));
        }

        let warmup = guard.config.timers.stuck_warmup_seconds;
        let (world_name, sx, sz) =
            guard
                .claims
                .get_safe_location(&origin.world, origin.block_x(), origin.block_z());
        let top = server
            .get_all_worlds()
            .into_iter()
            .find(|w| w.get_name() == world_name)
            .map(|w| w.get_top_block_y(sx, sz))
            .unwrap_or(origin.y as i32);

        let target = crate::factions::Location {
            world: world_name,
            x: sx as f64 + 0.5,
            y: top as f64 + 1.0,
            z: sz as f64 + 0.5,
            pitch: origin.pitch,
            yaw: origin.yaw,
        };

        let template = guard
            .raw
            .language
            .string("FACTIONS.STUCK_COMMAND.TASK_STARTED");
        guard.timers.set_pending(
            uuid,
            PendingAction {
                timer_type: TimerType::Stuck,
                expiry: crate::utils::now_millis() + (warmup as i64 * 1000),
                origin_world: origin.world.clone(),
                origin_x: origin.x,
                origin_y: origin.y,
                origin_z: origin.z,
                target: Some(target),
            },
        );

        messages = utils::replace(
            &template,
            "<time>",
            &utils::format_duration(warmup as i64 * 1000),
        );
    }
    reply(sender, messages);
}

fn toggle_open(sender: &CommandSender, server: &Server, uuid: &str) {
    let messages;
    {
        let mut guard = state().write().unwrap();
        let lang = guard.lang.clone();

        let Some(faction_id) = guard.factions.by_member(uuid).map(|f| f.id.clone()) else {
            reply(sender, &lang.faction_not_in);
            return;
        };
        let faction = guard.factions.get(&faction_id).cloned().unwrap();
        if !is_leader(&faction, uuid) {
            reply(sender, &lang.faction_leader_only);
            return;
        }

        let new_value = !faction.open;
        if let Some(faction) = guard.factions.get_mut(&faction_id) {
            faction.open = new_value;
        }
        messages = if new_value {
            utils::replace(&lang.faction_opened, "<name>", &faction.name)
        } else {
            utils::replace(&lang.faction_closed, "<name>", &faction.name)
        };
        let _ = server;
    }
    reply(sender, messages);
}

fn show(sender: &CommandSender, server: &Server, uuid: &str, name: Option<String>) {
    let guard = state().read().unwrap();
    let lang = guard.lang.clone();

    let faction = match &name {
        Some(name) => guard.factions.by_name(name).cloned(),
        None => guard.factions.by_member(uuid).cloned(),
    };

    let Some(faction) = faction else {
        reply(sender, &lang.faction_not_in);
        return;
    };

    // Relation colour for the faction name (as seen by the sender).
    let display = {
        let viewer = guard.factions.by_member(uuid);
        let color = match viewer {
            Some(v) if v.id == faction.id => &guard.config.relation_colors.teammate,
            Some(v) if v.is_ally(&faction.id) => &guard.config.relation_colors.ally,
            _ => &guard.config.relation_colors.enemy,
        };
        format!("{color}{}", faction.name)
    };

    let online = faction
        .members
        .values()
        .filter(|m| {
            pumpkin_plugin_api::uuid::parse(&m.uuid)
                .and_then(|id| server.get_player_by_uuid(id))
                .is_some()
        })
        .count();

    let role_names = |role: Role| -> String {
        let names: Vec<String> = faction
            .members
            .values()
            .filter(|m| m.role == role)
            .map(|m| {
                // Port of `getPlayerNameFormatted`: deathbanned = red, online = green,
                // offline = gray.
                let deathbanned = guard
                    .userdata
                    .get(&m.uuid)
                    .map(|u| u.is_deathbanned())
                    .unwrap_or(false);
                let online = pumpkin_plugin_api::uuid::parse(&m.uuid)
                    .and_then(|id| server.get_player_by_uuid(id))
                    .is_some();
                let color = if deathbanned {
                    "&c"
                } else if online {
                    "&a"
                } else {
                    "&7"
                };
                format!("{color}{}", m.name)
            })
            .collect();
        if names.is_empty() { lang.none.clone() } else { names.join("&7, ") }
    };

    let allies: Vec<String> = faction
        .allies
        .iter()
        .filter_map(|id| guard.factions.get(id).map(|f| f.name.clone()))
        .collect();

    // `showInformation` removes the announcement/auto-revive/lives lines for
    // viewers outside the faction, and empty role/allies lines for everyone.
    let is_same_faction = guard.factions.by_member(uuid).is_some_and(|f| f.id == faction.id);
    let frozen = guard.timers.is_frozen(&faction.id);
    let dtr_string = faction.dtr_string(&guard.config, frozen);
    let max_dtr_string = faction.max_dtr_string(&guard.config);
    let raidable = if faction.is_raidable() { lang.yes.clone() } else { lang.no.clone() };
    let has_role = |role: Role| faction.members.values().any(|m| m.role == role);

    let sep = format!("&7&m{}", " ".repeat(80));

    reply(sender, &sep);
    reply(sender, format!(" {display} &7[{online}/{}] &e- &eHome: &7{}", faction.members.len(), faction.home_string()));
    if is_same_faction
        && let Some(announcement) =
            faction.announcement.as_deref().filter(|announcement| !announcement.is_empty())
    {
        reply(sender, format!(" &eAnnouncement: &7{announcement}"));
    }
    if is_same_faction {
        reply(sender, format!(" &eAuto revive: &7{}", if faction.auto_revive { lang.yes.clone() } else { lang.no.clone() }));
        reply(sender, format!(" &eFaction lives: &7{}", faction.lives));
    }
    reply(sender, format!(" &eTotal Kills: &7{}", faction.kills));
    reply(sender, format!(" &ePoints: &7{}", faction.points));
    reply(sender, format!(" &eKoTH captures: &7{}", faction.koths_capped));
    reply(sender, format!(" &eBalance: &7${}", faction.balance));
    reply(sender, format!(" &eLeader: {}", role_names(Role::Leader)));
    if has_role(Role::CoLeader) {
        reply(sender, format!(" &eCo-leaders: {}", role_names(Role::CoLeader)));
    }
    if has_role(Role::Captain) {
        reply(sender, format!(" &eCaptains: {}", role_names(Role::Captain)));
    }
    if has_role(Role::Member) {
        reply(sender, format!(" &eMembers: {}", role_names(Role::Member)));
    }
    if !allies.is_empty() {
        reply(sender, format!(" &eAllies: {}", allies.join("&7, &f")));
    }
    // The `<regen-time>` line only shows while the faction DTR is frozen.
    if frozen {
        reply(sender, format!(
            " &eRegenerating: &7{}",
            utils::format_duration_words(guard.timers.freeze_remaining(&faction.id))
        ));
    }
    reply(sender, format!(
        " &eDTR: {dtr_string}&7/&a{max_dtr_string} &eRaidable: {}{raidable}",
        faction.dtr_color()
    ));
    reply(sender, &sep);
}

/// One line of a paginated/leaderboard listing: either plain chat text or a
/// clickable entry ("/f show <name>" with the show-hover).
enum ListLine {
    Plain(String),
    Entry { line: String, hover: String, command: String },
}

/// Port of `ListCommand`: factions with online members, 10 per page, sorted by
/// online count (descending). Entries carry the original hover/click events.
fn list(sender: &CommandSender, server: &Server, uuid: &str, page: u32) {
    // `SubCommand#forPlayersOnly` (console gets `COMMANDS.FOR_PLAYERS_ONLY`).
    let Some(player) = sender.as_player() else {
        let (prefix, template) = {
            let guard = state().read().unwrap();
            (
                guard.raw.language.string("FACTION_PREFIX"),
                guard.raw.language.string("COMMANDS.FOR_PLAYERS_ONLY"),
            )
        };
        reply(sender, format!("{prefix}{template}"));
        return;
    };

    // Host calls (online list) must happen before taking the state lock.
    let online_uuids: Vec<String> = server
        .get_all_players()
        .iter()
        .map(core::uuid_of)
        .collect();

    // Online count per faction; vanished players the viewer cannot see are
    // skipped (`!player.canSee(online) && vanishManager.isVanished(online)`).
    let (counts, viewer_faction) = {
        let guard = state().read().unwrap();
        let viewer_is_staff = guard.staff.is_staff(uuid);

        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for other in &online_uuids {
            if !viewer_is_staff && guard.staff.is_vanished(other) {
                continue;
            }
            if let Some(faction) = guard.factions.by_member(other) {
                *counts.entry(faction.id.clone()).or_default() += 1;
            }
        }

        (counts, guard.factions.by_member(uuid).map(|f| f.id.clone()))
    };

    let page_total = counts.len().div_ceil(10).max(1) as u32;

    if page > page_total {
        let (prefix, template) = {
            let guard = state().read().unwrap();
            (
                guard.raw.language.string("FACTION_PREFIX"),
                guard
                    .raw
                    .language
                    .string("FACTIONS.LIST_COMMAND.PAGE_NOT_FOUND"),
            )
        };
        reply(
            sender,
            format!("{prefix}{}", utils::replace(&template, "<number>", &page_total.to_string())),
        );
        return;
    }

    let mut sorted: Vec<(String, usize)> = counts.into_iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1));

    // Snapshot every line under a single lock; components are built afterwards
    // (creating text components is a host call).
    let lines = {
        let guard = state().read().unwrap();
        let mut lines: Vec<ListLine> = Vec::new();

        for template in guard.raw.language.string_list("FACTIONS.LIST_COMMAND.HEADER") {
            lines.push(ListLine::Plain(utils::replace(
                &utils::replace(&template, "<page>", &page.to_string()),
                "<pageTotal>",
                &page_total.to_string(),
            )));
        }

        let format = guard
            .raw
            .language
            .string("FACTIONS.LIST_COMMAND.FACTION_FORMAT");
        let hover_template = guard
            .raw
            .language
            .string("FACTIONS.SHOW_COMMAND.HOVER_TEXT");

        let start = (page as usize - 1) * 10;
        for index in start..start + 10 {
            let Some((faction_id, online_count)) = sorted.get(index) else {
                break;
            };
            let Some(faction) = guard.factions.get(faction_id) else {
                continue;
            };

            let name = core::faction_display_name_guard(
                &guard,
                faction,
                viewer_faction.as_deref(),
                &guard.config.relation_colors,
            );
            let dtr = faction.dtr_string(&guard.config, guard.timers.is_frozen(faction_id));
            let max_dtr = faction.max_dtr_string(&guard.config);

            let line = utils::replace_all(
                &format,
                &[
                    ("<number>", &(index + 1).to_string()),
                    ("<name>", &name),
                    ("<online-count>", &online_count.to_string()),
                    ("<faction-size>", &faction.members.len().to_string()),
                    ("<dtr>", &dtr),
                    ("<maxDtr>", &max_dtr),
                ],
            );

            lines.push(ListLine::Entry {
                line,
                hover: utils::replace(&hover_template, "<faction>", &faction.name),
                command: format!("/f show {}", faction.name),
            });
        }

        for template in guard.raw.language.string_list("FACTIONS.LIST_COMMAND.FOOTER") {
            lines.push(ListLine::Plain(template));
        }

        lines
    };

    for line in lines {
        match line {
            ListLine::Plain(text) => reply(sender, text),
            ListLine::Entry { line, hover, command } => {
                let component = utils::text(&line)
                    .click_run_command(&command)
                    .hover_show_text(utils::text(&hover));
                core::send_component(&player, component);
            }
        }
    }
}

/// `/f top` without a type prints the usage (`LEADERBOARDS.FACTION_COMMAND_USAGE`).
fn top_usage(sender: &CommandSender) {
    let (prefix, usage) = {
        let guard = state().read().unwrap();
        (
            guard.raw.language.string("PREFIX"),
            guard
                .raw
                .language
                .string("LEADERBOARDS.FACTION_COMMAND_USAGE"),
        )
    };

    reply(sender, format!("{prefix}{usage}"));
}

/// Port of `TopCommand`: `kills|points|balance|koths` faction leaderboards.
/// The original reads a 10-entry cache filled by faction data changes; the port
/// sorts the live faction values and skips zero entries (a faction only shows up
/// in the original after its value changed).
fn top(sender: &CommandSender, _server: &Server, uuid: &str, metric: &str) {
    let (title_key, format_key) = match metric.to_lowercase().as_str() {
        "kills" => (
            "LEADERBOARDS.FACTION_TOP_KILLS.TITLE",
            "LEADERBOARDS.FACTION_TOP_KILLS.LINE_FORMAT",
        ),
        "points" => (
            "LEADERBOARDS.FACTION_TOP_POINTS.TITLE",
            "LEADERBOARDS.FACTION_TOP_POINTS.LINE_FORMAT",
        ),
        "balance" => (
            "LEADERBOARDS.FACTION_TOP_BALANCE.TITLE",
            "LEADERBOARDS.FACTION_TOP_BALANCE.LINE_FORMAT",
        ),
        "koths" => (
            "LEADERBOARDS.FACTION_TOP_KOTHS_CAPPED.TITLE",
            "LEADERBOARDS.FACTION_TOP_KOTHS_CAPPED.LINE_FORMAT",
        ),
        _ => {
            let (prefix, template) = {
                let guard = state().read().unwrap();
                (
                    guard.raw.language.string("PREFIX"),
                    guard
                        .raw
                        .language
                        .string("LEADERBOARDS.TYPE_DOESNT_EXIST"),
                )
            };
            reply(sender, format!("{prefix}{}", utils::replace(&template, "<type>", metric)));
            return;
        }
    };

    let value_of = |faction: &Faction| -> i64 {
        match metric.to_lowercase().as_str() {
            "kills" => faction.kills as i64,
            "balance" => faction.balance,
            "koths" => faction.koths_capped as i64,
            _ => faction.points as i64,
        }
    };

    // Snapshot the entries + display names under one lock.
    let (header, title, line_format, footer, hover_template, entries) = {
        let guard = state().read().unwrap();
        let viewer_faction = guard.factions.by_member(uuid).map(|f| f.id.clone());

        let mut factions: Vec<&Faction> = guard
            .factions
            .player_factions()
            .filter(|faction| value_of(faction) != 0)
            .collect();
        factions.sort_by(|a, b| value_of(b).cmp(&value_of(a)).then_with(|| a.name.cmp(&b.name)));

        let entries: Vec<(String, String, i64)> = factions
            .into_iter()
            .take(10)
            .map(|faction| {
                (
                    core::faction_display_name_guard(
                        &guard,
                        faction,
                        viewer_faction.as_deref(),
                        &guard.config.relation_colors,
                    ),
                    faction.name.clone(),
                    value_of(faction),
                )
            })
            .collect();

        (
            guard.raw.language.string("LEADERBOARDS.COMMAND_HEADER"),
            guard.raw.language.string(title_key),
            guard.raw.language.string(format_key),
            guard.raw.language.string("LEADERBOARDS.COMMAND_FOOTER"),
            guard.raw.language.string("FACTIONS.SHOW_COMMAND.HOVER_TEXT"),
            entries,
        )
    };

    if entries.is_empty() {
        let (prefix, template) = {
            let guard = state().read().unwrap();
            (
                guard.raw.language.string("PREFIX"),
                guard.raw.language.string("LEADERBOARDS.NO_LEADERBOARDS"),
            )
        };
        reply(sender, format!("{prefix}{template}"));
        return;
    }

    reply(sender, header);
    reply(sender, title);

    let player = sender.as_player();
    for (index, (display, raw_name, value)) in entries.into_iter().enumerate() {
        let line = utils::replace_all(
            &line_format,
            &[
                ("<number>", &(index + 1).to_string()),
                ("<faction>", &display),
                ("<value>", &value.to_string()),
            ],
        );

        match &player {
            Some(player) => {
                let component = utils::text(&line)
                    .click_run_command(&format!("/f show {raw_name}"))
                    .hover_show_text(utils::text(&utils::replace(&hover_template, "<faction>", &display)));
                core::send_component(player, component);
            }
            // Console fallback: the original casts the sender to `Player` to
            // build the clickable line; from console the plain line is printed.
            None => reply(sender, line),
        }
    }

    reply(sender, footer);
}

/// `DepositCommand` / `WithdrawCommand`: FACTION_PREFIX + `DEPOSIT_COMMAND` /
/// `WITHDRAW_COMMAND` messages from the raw language; the result is broadcast
/// to the whole faction (the sender is a member, so it is included).
fn transfer_balance(
    sender: &CommandSender,
    server: &Server,
    uuid: &str,
    args: &ConsumedArgs,
    deposit: bool,
) {
    let amount_raw = present_string_arg(args, "amount").unwrap_or_default();

    let messages;
    {
        let mut guard = state().write().unwrap();
        let prefix = guard.raw.language.string("FACTION_PREFIX");

        if amount_raw.is_empty() {
            let usage = guard.raw.language.string(if deposit {
                "FACTIONS.DEPOSIT_COMMAND.USAGE"
            } else {
                "FACTIONS.WITHDRAW_COMMAND.USAGE"
            });
            drop(guard);
            return reply(sender, format!("{prefix}{usage}"));
        }

        let Some(id) = guard.factions.by_member(uuid).map(|f| f.id.clone()) else {
            let msg = guard.raw.language.string("FACTIONS.NOT_IN_FACTION_SELF");
            drop(guard);
            return reply(sender, format!("{prefix}{msg}"));
        };

        if !deposit {
            let role = guard.factions.get(&id).and_then(|f| f.member(uuid)).map(|m| m.role);
            if role.map(|r| r.is_at_least(Role::Captain)) != Some(true) {
                let tpl = guard.raw.language.string("FACTIONS.NO_PERMISSION");
                let msg = utils::replace(&tpl, "<role>", Role::Captain.name());
                drop(guard);
                return reply(sender, format!("{prefix}{msg}"));
            }
        }

        let user_balance = guard.userdata.get(uuid).map(|user| user.balance).unwrap_or(0);
        let faction_balance = guard.factions.get(&id).map(|f| f.balance).unwrap_or(0);

        // `all` -> the full balance; otherwise `Math.abs(isInteger ? parse : -)`.
        let amount = if amount_raw.eq_ignore_ascii_case("all") {
            if deposit { user_balance } else { faction_balance }
        } else {
            match amount_raw.trim().parse::<i64>() {
                Ok(value) => value.abs(),
                Err(_) => {
                    let invalid = guard.raw.language.string("COMMANDS.INVALID_NUMBER");
                    drop(guard);
                    return reply(sender, format!("{prefix}{invalid}"));
                }
            }
        };

        let max_balance = guard.config.economy.max_balance;

        if deposit {
            if amount > user_balance {
                let tpl = guard.raw.language.string("FACTIONS.DEPOSIT_COMMAND.NOT_ENOUGH_MONEY");
                let msg = utils::replace(&tpl, "<amount>", &user_balance.to_string());
                drop(guard);
                return reply(sender, format!("{prefix}{msg}"));
            }
            if amount == 0 {
                let msg = guard
                    .raw
                    .language
                    .string("FACTIONS.DEPOSIT_COMMAND.CAN_NOT_DEPOSIT_ZERO");
                drop(guard);
                return reply(sender, format!("{prefix}{msg}"));
            }

            if let Some(user) = guard.userdata.get_mut(uuid) {
                user.balance -= amount;
            }
            if let Some(faction) = guard.factions.get_mut(&id) {
                faction.balance = (faction.balance + amount).min(max_balance);
            }

            messages = format!(
                "{prefix}{}",
                utils::replace_all(
                    &guard.raw.language.string("FACTIONS.DEPOSIT_COMMAND.DEPOSITED"),
                    &[("<player>", &sender.get_name()), ("<amount>", &amount.to_string())],
                )
            );
        } else {
            if amount > faction_balance {
                let tpl = guard.raw.language.string("FACTIONS.WITHDRAW_COMMAND.NOT_ENOUGH_MONEY");
                let msg = utils::replace(&tpl, "<amount>", &faction_balance.to_string());
                drop(guard);
                return reply(sender, format!("{prefix}{msg}"));
            }

            if let Some(faction) = guard.factions.get_mut(&id) {
                faction.balance -= amount;
            }
            if let Some(user) = guard.userdata.get_mut(uuid) {
                user.balance = (user.balance + amount).min(max_balance);
            }

            messages = format!(
                "{prefix}{}",
                utils::replace_all(
                    &guard.raw.language.string("FACTIONS.WITHDRAW_COMMAND.WITHDRAWN"),
                    &[("<player>", &sender.get_name()), ("<amount>", &amount.to_string())],
                )
            );
        }

        broadcast_to_faction(server, &guard, &id, &messages);
    }
}

fn ally(sender: &CommandSender, server: &Server, uuid: &str, args: &ConsumedArgs, add: bool) {
    let Some(name) = present_string_arg(args, "faction") else {
        let usage = if add {
            state().read().unwrap().lang.usage_ally.clone()
        } else {
            state().read().unwrap().lang.usage_unally.clone()
        };
        reply(sender, usage);
        return;
    };

    let messages;
    {
        let mut guard = state().write().unwrap();
        let lang = guard.lang.clone();
        let max_allies = guard.config.factions.max_allies;

        let Some(my_id) = guard.factions.by_member(uuid).map(|f| f.id.clone()) else {
            reply(sender, &lang.faction_not_in);
            return;
        };
        let Some(other_id) = search_player_faction(&guard, &name) else {
            reply(sender, utils::replace(&lang.faction_doesnt_exist, "<argument>", &name));
            return;
        };
        if my_id == other_id {
            return;
        }

        if add {
            let ally_count = guard.factions.get(&my_id).map(|f| f.allies.len()).unwrap_or(0);
            if max_allies > 0 && ally_count >= max_allies {
                reply(sender, &lang.faction_ally_limit);
                return;
            }
            if let Some(faction) = guard.factions.get_mut(&my_id)
                && !faction.allies.contains(&other_id)
            {
                faction.allies.push(other_id.clone());
            }
            if let Some(faction) = guard.factions.get_mut(&other_id)
                && !faction.allies.contains(&my_id)
            {
                faction.allies.push(my_id.clone());
            }
            messages = utils::replace(&lang.faction_ally_added, "<faction>", &name);
        } else {
            if let Some(faction) = guard.factions.get_mut(&my_id) {
                faction.allies.retain(|id| id != &other_id);
            }
            if let Some(faction) = guard.factions.get_mut(&other_id) {
                faction.allies.retain(|id| id != &my_id);
            }
            messages = utils::replace(&lang.faction_ally_removed, "<faction>", &name);
        }
        let _ = server;
    }
    reply(sender, messages);
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn is_leader(faction: &Faction, uuid: &str) -> bool {
    faction.member(uuid).map(|m| m.role) == Some(Role::Leader)
}

fn is_connected(claims: &crate::factions::ClaimManager, faction_id: &str, world: &str, cx: i32, cz: i32) -> bool {
    let neighbors = [
        (cx + 1, cz),
        (cx - 1, cz),
        (cx, cz + 1),
        (cx, cz - 1),
        (cx + 1, cz + 1),
        (cx + 1, cz - 1),
        (cx - 1, cz + 1),
        (cx - 1, cz - 1),
    ];
    neighbors.iter().any(|(nx, nz)| {
        claims
            .faction_at(world, nx * 16, nz * 16)
            .map(|owner| owner == faction_id)
            .unwrap_or(false)
    })
}

// ---------------------------------------------------------------------------
// Admin subcommands
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum AdminField {
    Points,
    Balance,
    Lives,
    Kills,
}

#[derive(Clone, Copy)]
enum SystemFlag {
    Safezone,
    Enderpearls,
    Abilities,
}

/// Per-subcommand permission check (the original registers one
/// `lazarus.factions.*` node per admin subcommand).
fn require_permission(sender: &CommandSender, server: &Server, node: &str) -> bool {
    if sender.is_console() || sender.has_permission(server, node) {
        return true;
    }
    reply(sender, "&cYou do not have permission.");
    false
}

/// `AddDtrCommand` / `RemoveDtrCommand`: usage, `searchForFaction`,
/// `isDouble`, then `setDtr(current ± value)`; messages carry `getDtrString()`.
fn admin_change_dtr(sender: &CommandSender, server: &Server, args: &ConsumedArgs, add: bool) {
    if !require_permission(sender, server, "lazarus:factions.dtr") {
        return;
    }

    let mut guard = state().write().unwrap();
    let prefix = guard.raw.language.string("FACTION_PREFIX");

    let (Some(name), Some(value_raw)) = (
        present_string_arg(args, "faction"),
        present_string_arg(args, "value"),
    ) else {
        let usage = guard.raw.language.string(if add {
            "FACTIONS.ADD_DTR_COMMAND.USAGE"
        } else {
            "FACTIONS.REMOVE_DTR_COMMAND.USAGE"
        });
        drop(guard);
        return reply(sender, format!("{prefix}{usage}"));
    };

    let Some(id) = search_player_faction(&guard, &name) else {
        let tpl = guard.raw.language.string("FACTIONS.FACTION_DOESNT_EXIST");
        let msg = utils::replace(&tpl, "<argument>", &name);
        drop(guard);
        return reply(sender, format!("{prefix}{msg}"));
    };

    let value = match value_raw.trim().parse::<f64>() {
        Ok(value) if value.is_finite() => value,
        _ => {
            let invalid = guard.raw.language.string("COMMANDS.INVALID_NUMBER");
            drop(guard);
            return reply(sender, format!("{prefix}{invalid}"));
        }
    };

    let config = guard.config.clone();
    let (faction_name, members) = {
        let Some(faction) = guard.factions.get_mut(&id) else {
            return;
        };
        let amount = if add {
            faction.dtr + value
        } else {
            faction.dtr - value
        };
        faction.set_dtr(&config, amount);
        (
            faction.name.clone(),
            faction.members.values().map(|m| m.uuid.clone()).collect::<Vec<_>>(),
        )
    };
    let frozen = guard.timers.is_frozen(&id);
    let dtr_string = guard
        .factions
        .get(&id)
        .map(|faction| faction.dtr_string(&config, frozen))
        .unwrap_or_default();
    let sender_tpl = guard.raw.language.string(if add {
        "FACTIONS.ADD_DTR_COMMAND.CHANGED_SENDER"
    } else {
        "FACTIONS.REMOVE_DTR_COMMAND.CHANGED_SENDER"
    });
    let faction_tpl = guard.raw.language.string(if add {
        "FACTIONS.ADD_DTR_COMMAND.CHANGED_FACTION"
    } else {
        "FACTIONS.REMOVE_DTR_COMMAND.CHANGED_FACTION"
    });
    drop(guard);

    reply(
        sender,
        format!(
            "{prefix}{}",
            utils::replace_all(&sender_tpl, &[("<faction>", &faction_name), ("<value>", &dtr_string)])
        ),
    );

    let message = format!(
        "{prefix}{}",
        utils::replace_all(
            &faction_tpl,
            &[("<player>", &sender.get_name()), ("<dtr>", &dtr_string)]
        )
    );
    for uuid in members {
        if let Some(id) = pumpkin_plugin_api::uuid::parse(&uuid)
            && let Some(player) = server.get_player_by_uuid(id)
        {
            core::send(&player, &message);
        }
    }
}

fn admin_force_role(sender: &CommandSender, server: &Server, args: &ConsumedArgs, promote: bool) {
    if !require_permission(sender, server, if promote { "lazarus:factions.forcepromote" } else { "lazarus:factions.forcedemote" }) {
        return;
    }
    let Some(target_name) = present_string_arg(args, "player") else {
        return;
    };

    let (prefix, usage, not_in_tpl, max_tpl, min_tpl, leader_tpl, sender_tpl, faction_tpl) = {
        let guard = state().read().unwrap();
        let lang = &guard.raw.language;
        let (usage, max_tpl, min_tpl, leader_tpl, sender_tpl, faction_tpl) = if promote {
            (
                lang.string("FACTIONS.FORCE_PROMOTE_COMMAND.USAGE"),
                lang.string("FACTIONS.FORCE_PROMOTE_COMMAND.MAX_PROMOTE"),
                String::new(),
                String::new(),
                lang.string("FACTIONS.FORCE_PROMOTE_COMMAND.PROMOTED_SENDER"),
                lang.string("FACTIONS.FORCE_PROMOTE_COMMAND.PROMOTED_FACTION"),
            )
        } else {
            (
                lang.string("FACTIONS.FORCE_DEMOTE_COMMAND.USAGE"),
                String::new(),
                lang.string("FACTIONS.FORCE_DEMOTE_COMMAND.MIN_DEMOTE"),
                lang.string("FACTIONS.FORCE_DEMOTE_COMMAND.CANNOT_DEMOTE_LEADER"),
                lang.string("FACTIONS.FORCE_DEMOTE_COMMAND.DEMOTED_SENDER"),
                lang.string("FACTIONS.FORCE_DEMOTE_COMMAND.DEMOTED_FACTION"),
            )
        };
        (
            lang.string("FACTION_PREFIX"),
            usage,
            lang.string("FACTIONS.NOT_IN_FACTION"),
            max_tpl,
            min_tpl,
            leader_tpl,
            sender_tpl,
            faction_tpl,
        )
    };
    let _ = usage;

    // Locate the member by name across all player factions.
    let found = {
        let guard = state().read().unwrap();
        guard.factions.player_factions().find_map(|faction| {
            faction
                .members
                .values()
                .find(|m| m.name.eq_ignore_ascii_case(&target_name))
                .map(|m| (faction.id.clone(), m.uuid.clone(), m.role, faction.name.clone()))
        })
    };

    let Some((faction_id, member_uuid, role, faction_name)) = found else {
        let message = utils::replace(&not_in_tpl, "<player>", &target_name);
        reply(sender, format!("{prefix}{message}"));
        return;
    };

    let new_role = if promote {
        match role.promote() {
            Some(role) => role,
            None => {
                reply(sender, format!("{prefix}{max_tpl}"));
                return;
            }
        }
    } else {
        if role == Role::Leader {
            reply(sender, format!("{prefix}{leader_tpl}"));
            return;
        }
        match role.demote() {
            Some(role) => role,
            None => {
                reply(sender, format!("{prefix}{min_tpl}"));
                return;
            }
        }
    };

    let members = {
        let mut guard = state().write().unwrap();
        if let Some(faction) = guard.factions.get_mut(&faction_id) {
            if let Some(member) = faction.member_mut(&member_uuid) {
                member.role = new_role;
            }
            faction.members.values().map(|m| m.uuid.clone()).collect::<Vec<_>>()
        } else {
            Vec::new()
        }
    };

    let role_name = new_role.name();
    reply(
        sender,
        format!(
            "{prefix}{}",
            utils::replace_all(
                &sender_tpl,
                &[("<player>", &target_name), ("<role>", role_name)]
            )
        ),
    );

    let message = format!(
        "{prefix}{}",
        utils::replace_all(
            &faction_tpl,
            &[("<player>", &target_name), ("<role>", role_name)]
        )
    );
    let _ = faction_name;
    for uuid in members {
        if let Some(id) = pumpkin_plugin_api::uuid::parse(&uuid)
            && let Some(player) = server.get_player_by_uuid(id)
        {
            core::send(&player, &message);
        }
    }
}

fn admin_force_join(sender: &CommandSender, server: &Server, args: &ConsumedArgs) {
    if !require_permission(sender, server, "lazarus:factions.forcejoin") {
        return;
    }
    let Some(faction_name) = present_string_arg(args, "faction") else {
        let guard = state().read().unwrap();
        let prefix = guard.raw.language.string("FACTION_PREFIX");
        let usage = guard.raw.language.string("FACTIONS.FORCE_JOIN_COMMAND.USAGE");
        drop(guard);
        return reply(sender, format!("{prefix}{usage}"));
    };
    let Some(player) = sender.as_player() else {
        return;
    };
    let uuid = core::uuid_of(&player);
    let name = core::name_of(&player);

    let (prefix, missing_tpl, already_tpl, joined_tpl, already, found) = {
        let guard = state().read().unwrap();
        let lang = &guard.raw.language;
        let found = search_player_faction(&guard, &faction_name)
            .and_then(|id| guard.factions.get(&id))
            .map(|f| {
                (
                    f.id.clone(),
                    f.members.values().map(|m| m.uuid.clone()).collect::<Vec<_>>(),
                )
            });
        (
            lang.string("FACTION_PREFIX"),
            lang.string("FACTIONS.FACTION_DOESNT_EXIST"),
            lang.string("FACTIONS.ALREADY_IN_FACTION_SELF"),
            lang.string("FACTIONS.FORCE_JOIN_COMMAND.JOINED"),
            guard.factions.by_member(&uuid).is_some(),
            found,
        )
    };

    if already {
        reply(sender, format!("{prefix}{already_tpl}"));
        return;
    }
    let Some((id, members)) = found else {
        let message = utils::replace(&missing_tpl, "<argument>", &faction_name);
        reply(sender, format!("{prefix}{message}"));
        return;
    };

    {
        let mut guard = state().write().unwrap();
        guard
            .factions
            .add_member(&id, member(uuid.clone(), name.clone(), Role::Member));
    }

    let message = format!(
        "{prefix}{}",
        utils::replace(&joined_tpl, "<player>", &name)
    );
    for member_uuid in members {
        if let Some(id) = pumpkin_plugin_api::uuid::parse(&member_uuid)
            && let Some(online) = server.get_player_by_uuid(id)
        {
            core::send(&online, &message);
        }
    }
}

fn admin_force_rename(sender: &CommandSender, server: &Server, args: &ConsumedArgs) {
    let _ = server;
    if !require_permission(sender, server, "lazarus:factions.forcerename") {
        return;
    }
    let Some(old_arg) = present_string_arg(args, "faction") else {
        let guard = state().read().unwrap();
        let prefix = guard.raw.language.string("FACTION_PREFIX");
        let usage = guard.raw.language.string("FACTIONS.FORCE_RENAME_COMMAND.USAGE");
        drop(guard);
        return reply(sender, format!("{prefix}{usage}"));
    };
    let Some(new_name) = present_string_arg(args, "name") else {
        let guard = state().read().unwrap();
        let prefix = guard.raw.language.string("FACTION_PREFIX");
        let usage = guard.raw.language.string("FACTIONS.FORCE_RENAME_COMMAND.USAGE");
        drop(guard);
        return reply(sender, format!("{prefix}{usage}"));
    };

    let mut guard = state().write().unwrap();
    let config = guard.config.clone();

    let (prefix, usage, missing_tpl, same_tpl, blocked, too_short, too_big, not_alnum, exists_tpl, renamed_tpl) = {
        let lang = &guard.raw.language;
        (
            lang.string("FACTION_PREFIX"),
            lang.string("FACTIONS.FORCE_RENAME_COMMAND.USAGE"),
            lang.string("FACTIONS.FACTION_DOESNT_EXIST"),
            lang.string("FACTIONS.FORCE_RENAME_COMMAND.SAME_NAME"),
            lang.string("FACTIONS.BLOCKED_FACTION_NAME"),
            lang.string("FACTIONS.NAME_TOO_SHORT"),
            lang.string("FACTIONS.NAME_TOO_BIG"),
            lang.string("FACTIONS.NAME_NOT_ALPHANUMERIC"),
            lang.string("FACTIONS.FACTION_ALREADY_EXISTS"),
            lang.string("FACTIONS.FORCE_RENAME_COMMAND.RENAMED"),
        )
    };
    let _ = usage;

    let Some(id) = search_player_faction(&guard, &old_arg) else {
        let message = utils::replace(&missing_tpl, "<argument>", &old_arg);
        reply(sender, format!("{prefix}{message}"));
        return;
    };

    let old_name = guard.factions.get(&id).map(|f| f.name.clone()).unwrap_or_default();

    if old_name.eq_ignore_ascii_case(&new_name) {
        reply(sender, format!("{prefix}{}", utils::replace(&same_tpl, "<name>", &new_name)));
        return;
    }
    if config
        .factions
        .disallowed_names
        .iter()
        .any(|n| n.eq_ignore_ascii_case(&new_name))
    {
        reply(sender, format!("{prefix}{blocked}"));
        return;
    }
    if new_name.chars().count() < config.factions.name_min_length {
        reply(sender, format!("{prefix}{too_short}"));
        return;
    }
    if new_name.chars().count() > config.factions.name_max_length {
        reply(sender, format!("{prefix}{too_big}"));
        return;
    }
    if !new_name.chars().all(|c| c.is_ascii_alphanumeric()) {
        reply(sender, format!("{prefix}{not_alnum}"));
        return;
    }
    if guard.factions.name_taken(&new_name) {
        reply(sender, format!("{prefix}{}", utils::replace(&exists_tpl, "<name>", &new_name)));
        return;
    }

    if let Some(faction) = guard.factions.get_mut(&id) {
        faction.name = new_name.clone();
    }
    drop(guard);

    let message = utils::replace_all(
        &renamed_tpl,
        &[("<name>", &old_name), ("<newName>", &new_name)],
    );
    for player in server.get_all_players() {
        core::send(&player, &message);
    }
}

fn admin_system_create(sender: &CommandSender, server: &Server, args: &ConsumedArgs) {
    let _ = server;
    if !require_permission(sender, server, "lazarus:factions.systemcreate") {
        return;
    }
    let Some(name) = present_string_arg(args, "name") else {
        let guard = state().read().unwrap();
        let prefix = guard.raw.language.string("FACTION_PREFIX");
        let usage = guard.raw.language.string("FACTIONS.SYSTEM_CREATE_COMMAND.USAGE");
        drop(guard);
        return reply(sender, format!("{prefix}{usage}"));
    };

    let mut guard = state().write().unwrap();
    let prefix = guard.raw.language.string("FACTION_PREFIX");
    if guard.factions.name_taken(&name) {
        let tpl = guard.raw.language.string("FACTIONS.FACTION_ALREADY_EXISTS");
        drop(guard);
        reply(sender, format!("{prefix}{}", utils::replace(&tpl, "<name>", &name)));
        return;
    }
    let created = guard.raw.language.string("FACTIONS.SYSTEM_CREATE_COMMAND.CREATED");

    let id = crate::factions::new_id();
    let faction = Faction::new_system(
        id.clone(),
        name.clone(),
        crate::factions::FactionKind::System,
        "&7".to_string(),
    );
    guard.factions.factions.insert(id, faction);
    drop(guard);

    reply(
        sender,
        format!("{prefix}{}", utils::replace(&created, "<name>", &name)),
    );
}

fn admin_system_list(sender: &CommandSender) {
    fn kind_order(kind: crate::factions::FactionKind) -> u8 {
        match kind {
            crate::factions::FactionKind::System => 0,
            crate::factions::FactionKind::Safezone => 1,
            crate::factions::FactionKind::Warzone => 2,
            crate::factions::FactionKind::Wilderness => 3,
            crate::factions::FactionKind::Road => 4,
            crate::factions::FactionKind::Player => 5,
        }
    }

    let (header, format_tpl, footer, factions) = {
        let guard = state().read().unwrap();
        let lang = &guard.raw.language;
        let mut list: Vec<(u8, String, String)> = guard
            .factions
            .factions
            .values()
            .filter(|f| f.is_system())
            .map(|f| (kind_order(f.kind), f.color.clone(), f.name.clone()))
            .collect();
        list.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.2.cmp(&b.2)));
        (
            lang.string_list("FACTIONS.SYSTEM_LIST_COMMAND.HEADER"),
            lang.string("FACTIONS.SYSTEM_LIST_COMMAND.FACTION_FORMAT"),
            lang.string("FACTIONS.SYSTEM_LIST_COMMAND.FOOTER"),
            list,
        )
    };

    let count = factions.len().to_string();
    for line in &header {
        reply(sender, utils::replace(line, "<count>", &count));
    }
    for (index, (_, color, name)) in factions.iter().enumerate() {
        let number = (index + 1).to_string();
        let display = format!("{color}{name}");
        reply(
            sender,
            utils::replace_all(
                &format_tpl,
                &[("<number>", &number), ("<name>", &display)],
            ),
        );
    }
    reply(sender, footer);
}

fn admin_system_rename(sender: &CommandSender, server: &Server, args: &ConsumedArgs) {
    let _ = server;
    if !require_permission(sender, server, "lazarus:factions.systemrename") {
        return;
    }
    let Some(old_arg) = present_string_arg(args, "faction") else {
        let guard = state().read().unwrap();
        let prefix = guard.raw.language.string("FACTION_PREFIX");
        let usage = guard.raw.language.string("FACTIONS.SYSTEM_RENAME_COMMAND.USAGE");
        drop(guard);
        return reply(sender, format!("{prefix}{usage}"));
    };
    let Some(new_name) = present_string_arg(args, "name") else {
        let guard = state().read().unwrap();
        let prefix = guard.raw.language.string("FACTION_PREFIX");
        let usage = guard.raw.language.string("FACTIONS.SYSTEM_RENAME_COMMAND.USAGE");
        drop(guard);
        return reply(sender, format!("{prefix}{usage}"));
    };

    let mut guard = state().write().unwrap();
    let prefix = guard.raw.language.string("FACTION_PREFIX");
    let missing_tpl = guard.raw.language.string("FACTIONS.FACTION_DOESNT_EXIST");
    let not_system = guard.raw.language.string("FACTIONS.SYSTEM_RENAME_COMMAND.NOT_SYSTEM_FACTION");
    let cannot = guard
        .raw
        .language
        .string("FACTIONS.SYSTEM_RENAME_COMMAND.CANNOT_RENAME_THIS_TYPE");
    let same_tpl = guard.raw.language.string("FACTIONS.SYSTEM_RENAME_COMMAND.SAME_NAME");
    let exists_tpl = guard.raw.language.string("FACTIONS.FACTION_ALREADY_EXISTS");
    let renamed_tpl = guard.raw.language.string("FACTIONS.SYSTEM_RENAME_COMMAND.RENAMED");

    let Some(id) = guard.factions.by_name(&old_arg).map(|f| f.id.clone()) else {
        reply(sender, format!("{prefix}{}", utils::replace(&missing_tpl, "<argument>", &old_arg)));
        return;
    };

    let Some(faction) = guard.factions.get(&id) else {
        return;
    };
    let kind = faction.kind;
    let old_name = faction.name.clone();
    let color = faction.color.clone();

    if kind == crate::factions::FactionKind::Player {
        reply(sender, format!("{prefix}{not_system}"));
        return;
    }
    if kind != crate::factions::FactionKind::System {
        reply(sender, format!("{prefix}{cannot}"));
        return;
    }
    if old_name.eq_ignore_ascii_case(&new_name) {
        reply(sender, format!("{prefix}{}", utils::replace(&same_tpl, "<name>", &new_name)));
        return;
    }
    if guard.factions.name_taken(&new_name) {
        reply(sender, format!("{prefix}{}", utils::replace(&exists_tpl, "<name>", &new_name)));
        return;
    }

    if let Some(faction) = guard.factions.get_mut(&id) {
        faction.name = new_name.clone();
    }
    drop(guard);

    reply(
        sender,
        format!(
            "{prefix}{}",
            utils::replace_all(
                &renamed_tpl,
                &[("<faction>", &format!("{color}{old_name}")), ("<name>", &format!("{color}{new_name}"))],
            )
        ),
    );
}

fn admin_toggle_system_flag(
    sender: &CommandSender,
    server: &Server,
    args: &ConsumedArgs,
    flag: SystemFlag,
) {
    let _ = server;
    let node = match flag {
        SystemFlag::Safezone => "lazarus:factions.safezone",
        SystemFlag::Enderpearls => "lazarus:factions.enderpearls",
        SystemFlag::Abilities => "lazarus:factions.abilities",
    };
    if !require_permission(sender, server, node) {
        return;
    }
    let Some(name) = present_string_arg(args, "faction") else {
        return;
    };

    let mut guard = state().write().unwrap();
    let lang = &guard.raw.language;
    let prefix = lang.string("FACTION_PREFIX");
    let usage = lang.string(match flag {
        SystemFlag::Safezone => "FACTIONS.SAFEZONE_COMMAND.USAGE",
        SystemFlag::Enderpearls => "FACTIONS.ENDERPEARLS_COMMAND.USAGE",
        SystemFlag::Abilities => "FACTIONS.ABILITIES_COMMAND.USAGE",
    });
    let missing_tpl = lang.string("FACTIONS.FACTION_DOESNT_EXIST");
    let not_system = lang.string(match flag {
        SystemFlag::Safezone => "FACTIONS.SAFEZONE_COMMAND.NOT_SYSTEM_FACTION",
        SystemFlag::Enderpearls => "FACTIONS.ENDERPEARLS_COMMAND.NOT_SYSTEM_FACTION",
        SystemFlag::Abilities => "FACTIONS.ABILITIES_COMMAND.NOT_SYSTEM_FACTION",
    });

    let Some(id) = guard.factions.by_name(&name).map(|f| f.id.clone()) else {
        let message = utils::replace(&missing_tpl, "<argument>", &name);
        reply(sender, format!("{prefix}{message}"));
        return;
    };

    let Some(faction) = guard.factions.get(&id) else {
        return;
    };
    if !faction.is_system() {
        reply(sender, format!("{prefix}{not_system}"));
        return;
    }
    let display = format!("{}{}", faction.color, faction.name);

    // Toggle and render the option-specific message.
    let message = match flag {
        SystemFlag::Safezone => {
            let faction = guard.factions.get_mut(&id).unwrap();
            faction.safezone = !faction.safezone;
            let now = faction.safezone;
            let tpl = guard.raw.language.string("FACTIONS.SAFEZONE_COMMAND.CHANGED");
            utils::replace_all(
                &tpl,
                &[
                    ("<faction>", &display),
                    ("<safezone>", if now { "&atrue" } else { "&cfalse" }),
                ],
            )
        }
        SystemFlag::Enderpearls => {
            let faction = guard.factions.get_mut(&id).unwrap();
            faction.enderpearls = !faction.enderpearls;
            let now = faction.enderpearls;
            let tpl = guard.raw.language.string(if now {
                "FACTIONS.ENDERPEARLS_COMMAND.ENABLED"
            } else {
                "FACTIONS.ENDERPEARLS_COMMAND.DISABLED"
            });
            utils::replace(&tpl, "<faction>", &display)
        }
        SystemFlag::Abilities => {
            let faction = guard.factions.get_mut(&id).unwrap();
            faction.abilities = !faction.abilities;
            let now = faction.abilities;
            let tpl = guard.raw.language.string(if now {
                "FACTIONS.ABILITIES_COMMAND.ENABLED"
            } else {
                "FACTIONS.ABILITIES_COMMAND.DISABLED"
            });
            utils::replace(&tpl, "<faction>", &display)
        }
    };
    let _ = usage;
    drop(guard);

    reply(sender, format!("{prefix}{message}"));
}

fn admin_color(sender: &CommandSender, server: &Server, args: &ConsumedArgs) {
    if !require_permission(sender, server, "lazarus:factions.color") {
        return;
    }
    let Some(name) = present_string_arg(args, "faction") else {
        let guard = state().read().unwrap();
        let prefix = guard.raw.language.string("FACTION_PREFIX");
        let usage = guard.raw.language.string("FACTIONS.COLOR_COMMAND.USAGE");
        drop(guard);
        return reply(sender, format!("{prefix}{usage}"));
    };
    let Some(color) = present_string_arg(args, "color") else {
        let guard = state().read().unwrap();
        let prefix = guard.raw.language.string("FACTION_PREFIX");
        let usage = guard.raw.language.string("FACTIONS.COLOR_COMMAND.USAGE");
        drop(guard);
        return reply(sender, format!("{prefix}{usage}"));
    };

    let mut guard = state().write().unwrap();
    let lang = &guard.raw.language;
    let prefix = lang.string("FACTION_PREFIX");
    let missing_tpl = lang.string("FACTIONS.FACTION_DOESNT_EXIST");
    let not_system = lang.string("FACTIONS.COLOR_COMMAND.NOT_SYSTEM_FACTION");
    let invalid_tpl = lang.string("FACTIONS.COLOR_COMMAND.COLOR_DOESNT_EXIST");
    let changed_tpl = lang.string("FACTIONS.COLOR_COMMAND.CHANGED");

    let Some(id) = guard.factions.by_name(&name).map(|f| f.id.clone()) else {
        let message = utils::replace(&missing_tpl, "<argument>", &name);
        reply(sender, format!("{prefix}{message}"));
        return;
    };

    let Some(faction) = guard.factions.get(&id) else {
        return;
    };
    if !faction.is_system() {
        reply(sender, format!("{prefix}{not_system}"));
        return;
    }

    let lowered = color.to_lowercase();
    if !is_valid_color(&lowered) {
        reply(sender, format!("{prefix}{}", utils::replace(&invalid_tpl, "<color>", &color)));
        return;
    }

    let old_display = format!("{}{}", faction.color, faction.name);
    if let Some(faction) = guard.factions.get_mut(&id) {
        faction.color = lowered.clone();
    }
    let new_display = format!("{}{}", lowered, guard.factions.get(&id).map(|f| f.name.clone()).unwrap_or_default());

    reply(
        sender,
        format!(
            "{prefix}{}",
            utils::replace_all(&changed_tpl, &[("<faction>", &old_display), ("<color>", &new_display)])
        ),
    );
}

/// Port of `ColorCommand#isColorStringValid`.
fn is_valid_color(value: &str) -> bool {
    if value.is_empty() || value.len() % 2 != 0 {
        return false;
    }
    let bytes = value.as_bytes();
    for pair in bytes.chunks(2) {
        if pair[0] != b'&' {
            return false;
        }
        let c = pair[1] as char;
        if !"0123456789abcdefklmnor".contains(c) {
            return false;
        }
    }
    true
}

fn admin_deathban(sender: &CommandSender, server: &Server, args: &ConsumedArgs) {
    let _ = server;
    if !require_permission(sender, server, "lazarus:factions.deathban") {
        return;
    }
    let Some(name) = present_string_arg(args, "faction") else {
        let guard = state().read().unwrap();
        let prefix = guard.raw.language.string("FACTION_PREFIX");
        let usage = guard.raw.language.string("FACTIONS.DEATHBAN_COMMAND.USAGE");
        drop(guard);
        return reply(sender, format!("{prefix}{usage}"));
    };

    let mut guard = state().write().unwrap();
    let lang = &guard.raw.language;
    let prefix = lang.string("FACTION_PREFIX");
    let missing_tpl = lang.string("FACTIONS.FACTION_DOESNT_EXIST");
    let not_system = lang.string("FACTIONS.DEATHBAN_COMMAND.NOT_SYSTEM_FACTION");
    let changed_tpl = lang.string("FACTIONS.DEATHBAN_COMMAND.CHANGED");

    let Some(id) = guard.factions.by_name(&name).map(|f| f.id.clone()) else {
        let message = utils::replace(&missing_tpl, "<argument>", &name);
        reply(sender, format!("{prefix}{message}"));
        return;
    };

    let Some(faction) = guard.factions.get(&id) else {
        return;
    };
    if !faction.is_system() {
        reply(sender, format!("{prefix}{not_system}"));
        return;
    }
    let display = format!("{}{}", faction.color, faction.name);

    let Some(faction) = guard.factions.get_mut(&id) else {
        return;
    };
    faction.deathban = !faction.deathban;
    let deathban = if faction.deathban { "&cDeathban" } else { "&aNon-Deathban" };

    reply(
        sender,
        format!(
            "{prefix}{}",
            utils::replace_all(&changed_tpl, &[("<faction>", &display), ("<deathban>", deathban)])
        ),
    );
}

fn admin_chat_spy(sender: &CommandSender, server: &Server) {
    let _ = server;
    if !require_permission(sender, server, "lazarus:factions.chatspy") {
        return;
    }
    let Some(player) = sender.as_player() else {
        return;
    };
    let uuid = core::uuid_of(&player);

    let (enabled, message) = {
        let mut guard = state().write().unwrap();
        let enabled = guard.factions.toggle_chat_spy(&uuid);
        let key = if enabled {
            "FACTIONS.CHATSPY_ENABLED"
        } else {
            "FACTIONS.CHATSPY_DISABLED"
        };
        let prefix = guard.raw.language.string("FACTION_PREFIX");
        let message = guard.raw.language.string(key);
        (enabled, format!("{prefix}{message}"))
    };
    let _ = enabled;
    reply(sender, message);
}

fn admin_tp_here(sender: &CommandSender, server: &Server, args: &ConsumedArgs) {
    if !require_permission(sender, server, "lazarus:factions.tphere") {
        return;
    }
    let Some(name) = present_string_arg(args, "faction") else {
        let guard = state().read().unwrap();
        let prefix = guard.raw.language.string("FACTION_PREFIX");
        let usage = guard.raw.language.string("FACTIONS.TP_HERE_COMMAND.USAGE");
        drop(guard);
        return reply(sender, format!("{prefix}{usage}"));
    };
    let Some(player) = sender.as_player() else {
        return;
    };

    let (prefix, missing_tpl, faction_tpl, sender_tpl, members, faction_name) = {
        let guard = state().read().unwrap();
        let lang = &guard.raw.language;
        let found = search_player_faction(&guard, &name)
            .and_then(|id| guard.factions.get(&id))
            .map(|f| {
                (
                    f.name.clone(),
                    f.members.values().map(|m| m.uuid.clone()).collect::<Vec<_>>(),
                )
            });
        let (faction_name, members) = found.unwrap_or_default();
        (
            lang.string("FACTION_PREFIX"),
            lang.string("FACTIONS.FACTION_DOESNT_EXIST"),
            lang.string("FACTIONS.TP_HERE_COMMAND.TELEPORTED_FACTION"),
            lang.string("FACTIONS.TP_HERE_COMMAND.TELEPORTED_SENDER"),
            members,
            faction_name,
        )
    };

    if faction_name.is_empty() {
        let message = utils::replace(&missing_tpl, "<argument>", &name);
        reply(sender, format!("{prefix}{message}"));
        return;
    }

    let location = core::location(&player);
    let position = (location.x, location.y, location.z);

    for member_uuid in &members {
        if let Some(id) = pumpkin_plugin_api::uuid::parse(member_uuid)
            && let Some(online) = server.get_player_by_uuid(id)
        {
            if let Some(world) = server
                .get_all_worlds()
                .into_iter()
                .find(|w| w.get_name() == location.world)
            {
                online.teleport(position, None, None, world);
            }
            core::send(
                &online,
                format!("{prefix}{}", utils::replace(&faction_tpl, "<player>", &core::name_of(&player))),
            );
        }
    }

    reply(
        sender,
        format!("{prefix}{}", utils::replace(&sender_tpl, "<faction>", &faction_name)),
    );
}

fn admin_remove_claim(sender: &CommandSender, server: &Server) {
    let _ = server;
    if !require_permission(sender, server, "lazarus:factions.removeclaim") {
        return;
    }
    let Some(player) = sender.as_player() else {
        return;
    };
    let location = core::location(&player);

    let mut guard = state().write().unwrap();
    let lang_prefix = guard.raw.language.string("FACTION_PREFIX");

    let Some((owner_id, claim)) = guard
        .claims
        .claim_at(&location.world, location.block_x(), location.block_z())
        .map(|(owner, claim)| (owner.clone(), claim.clone()))
    else {
        let message = guard.raw.language.string("FACTIONS.REMOVE_CLAIM_COMMAND.NO_CLAIM");
        drop(guard);
        reply(sender, format!("{lang_prefix}{message}"));
        return;
    };

    let center = format!("{}, {}", (claim.min_x + claim.max_x) / 2, (claim.min_z + claim.max_z) / 2);
    let owner_display = guard
        .factions
        .get(&owner_id)
        .map(|f| format!("{}{}", f.color, f.name))
        .unwrap_or_default();
    let is_player = guard.factions.get(&owner_id).map(|f| f.is_player()).unwrap_or(false);

    guard
        .claims
        .remove_claim_at(&owner_id, &location.world, location.block_x(), location.block_z());

    // Clear the home if it was inside the removed claim.
    if is_player
        && claim.contains(&location.world, claim.min_x, claim.min_z)
        && let Some(faction) = guard.factions.get(&owner_id)
        && let Some(home) = &faction.home
        && claim.contains(&location.world, home.block_x(), home.block_z())
        && let Some(faction) = guard.factions.get_mut(&owner_id)
    {
        faction.home = None;
    }

    let sender_tpl = guard.raw.language.string("FACTIONS.REMOVE_CLAIM_COMMAND.REMOVED_SENDER");
    let faction_tpl = guard.raw.language.string("FACTIONS.REMOVE_CLAIM_COMMAND.REMOVED_FACTION");
    let members: Vec<String> = if is_player {
        guard
            .factions
            .get(&owner_id)
            .map(|f| f.members.values().map(|m| m.uuid.clone()).collect())
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    drop(guard);

    reply(
        sender,
        format!(
            "{lang_prefix}{}",
            utils::replace_all(&sender_tpl, &[("<faction>", &owner_display), ("<location>", &center)])
        ),
    );

    if is_player {
        let player_name = core::name_of(&player);
        let message = format!(
            "{lang_prefix}{}",
            utils::replace_all(&faction_tpl, &[("<player>", &player_name), ("<location>", &center)])
        );
        for member_uuid in members {
            if let Some(id) = pumpkin_plugin_api::uuid::parse(&member_uuid)
                && let Some(online) = server.get_player_by_uuid(id)
            {
                core::send(&online, &message);
            }
        }
    }
}

fn admin_claim_for(sender: &CommandSender, server: &Server, args: &ConsumedArgs) {
    let _ = server;
    if !require_permission(sender, server, "lazarus:factions.claimfor") {
        return;
    }
    let Some(name) = present_string_arg(args, "faction") else {
        let guard = state().read().unwrap();
        let prefix = guard.raw.language.string("FACTION_PREFIX");
        let usage = guard.raw.language.string("FACTIONS.CLAIM_FOR_COMMAND.USAGE");
        drop(guard);
        return reply(sender, format!("{prefix}{usage}"));
    };
    let Some(player) = sender.as_player() else {
        return;
    };
    let uuid = core::uuid_of(&player);

    let mut guard = state().write().unwrap();
    let prefix = guard.raw.language.string("FACTION_PREFIX");

    let Some(owner_id) = guard
        .factions
        .by_name(&name)
        .filter(|f| f.is_system())
        .map(|f| f.id.clone())
    else {
        let message = utils::replace(
            &guard.raw.language.string("FACTIONS.FACTION_DOESNT_EXIST"),
            "<argument>",
            &name,
        );
        drop(guard);
        reply(sender, format!("{prefix}{message}"));
        return;
    };

    // Start (or continue) a SYSTEM_CLAIM selection bound to this faction. A missing,
    // partial or differently-bound selection means we (re)hand out the wand.
    let bound = matches!(
        guard.selection.get(&uuid).map(|s| s.kind.clone()),
        Some(crate::selection::SelectionKind::System(id)) if id == owner_id
    );
    if !bound {
        let message = guard.raw.language.string("FACTIONS.CLAIM_FOR_COMMAND.MAKE_A_SELECTION");
        guard.selection.start_system(&player, owner_id.clone());
        drop(guard);
        reply(sender, format!("{prefix}{message}"));
        return;
    }

    let Some(claim) = guard.selection.get(&uuid).and_then(|s| s.to_claim()) else {
        let message = guard
            .raw
            .language
            .string("FACTIONS.CLAIM_FOR_COMMAND.SET_BOTH_POSITIONS");
        drop(guard);
        reply(sender, format!("{prefix}{message}"));
        return;
    };

    if guard.claims.overlaps(&claim) {
        let message = guard.raw.language.string("FACTIONS.CLAIM_FOR_COMMAND.CLAIM_OVERLAPPING");
        drop(guard);
        reply(sender, format!("{prefix}{message}"));
        return;
    }

    let claim_clone = claim.clone();
    let owner_id_clone = owner_id.clone();
    guard.claims.add_claim(&owner_id, claim);
    let display = guard
        .factions
        .get(&owner_id)
        .map(|f| format!("{}{}", f.color, f.name))
        .unwrap_or_default();
    let message = utils::replace_all(
        &guard.raw.language.string("FACTIONS.CLAIM_FOR_COMMAND.CLAIM_CLAIMED"),
        &[("<player>", &core::name_of(&player)), ("<faction>", &display)],
    );
    guard.selection.cancel_local(&player);
    guard
        .glass
        .clear_type(&player, crate::glass::GlassType::ClaimSelection);
    drop(guard);

    // Eject PvP-protected players that ended up inside the new claim.
    crate::utils::core::eject_intruders(server, &claim_clone, &owner_id_clone);

    reply(sender, format!("{prefix}{message}"));
}

fn friendly_fire(sender: &CommandSender, server: &Server) {
    let Some(player) = sender.as_player() else {
        return;
    };
    let uuid = core::uuid_of(&player);

    let (message, members) = {
        let mut guard = state().write().unwrap();
        let prefix = guard.raw.language.string("FACTION_PREFIX");
        let not_in_faction = guard.raw.language.string("FACTIONS.NOT_IN_FACTION_SELF");
        let no_permission_tpl = guard.raw.language.string("FACTIONS.NO_PERMISSION");
        let on_tpl = guard.raw.language.string("FACTIONS.FRIENDLY_FIRE_TOGGLED_ON");
        let off_tpl = guard.raw.language.string("FACTIONS.FRIENDLY_FIRE_TOGGLED_OFF");

        let Some(id) = guard.factions.by_member(&uuid).map(|f| f.id.clone()) else {
            return reply(sender, format!("{prefix}{not_in_faction}"));
        };

        let role = guard.factions.get(&id).and_then(|f| f.member(&uuid)).map(|m| m.role);
        if role.map(|r| r.is_at_least(Role::CoLeader)) != Some(true) {
            let message = utils::replace(&no_permission_tpl, "<role>", Role::CoLeader.name());
            return reply(sender, format!("{prefix}{message}"));
        }

        let faction = guard.factions.get_mut(&id).unwrap();
        faction.friendly_fire = !faction.friendly_fire;
        let template = if faction.friendly_fire { on_tpl } else { off_tpl };
        let members = guard
            .factions
            .get(&id)
            .unwrap()
            .members
            .values()
            .map(|m| m.uuid.clone())
            .collect::<Vec<_>>();
        (format!("{prefix}{template}"), members)
    };

    for member_uuid in members {
        if let Some(id) = pumpkin_plugin_api::uuid::parse(&member_uuid)
            && let Some(online) = server.get_player_by_uuid(id)
        {
            core::send(&online, &message);
        }
    }
}

fn uninvite(sender: &CommandSender, server: &Server, args: &ConsumedArgs) {
    let Some(player) = sender.as_player() else {
        return;
    };
    let uuid = core::uuid_of(&player);

    let mut guard = state().write().unwrap();
    let prefix = guard.raw.language.string("FACTION_PREFIX");
    let not_in_faction = guard.raw.language.string("FACTIONS.NOT_IN_FACTION_SELF");
    let no_permission_tpl = guard.raw.language.string("FACTIONS.NO_PERMISSION");
    let usage = guard.raw.language.string("FACTIONS.UNINVITE_COMMAND.USAGE");
    let all_tpl = guard.raw.language.string("FACTIONS.UNINVITE_COMMAND.ALL");
    let uninvited_tpl = guard.raw.language.string("FACTIONS.UNINVITE_COMMAND.UNINVITED");
    let not_invited_tpl = guard.raw.language.string("FACTIONS.UNINVITE_COMMAND.NOT_INVITED");

    let Some(id) = guard.factions.by_member(&uuid).map(|f| f.id.clone()) else {
        drop(guard);
        return reply(sender, format!("{prefix}{not_in_faction}"));
    };

    let Some(target) = present_string_arg(args, "player") else {
        drop(guard);
        return reply(sender, format!("{prefix}{usage}"));
    };

    let role = guard.factions.get(&id).and_then(|f| f.member(&uuid)).map(|m| m.role);
    if role.map(|r| r.is_at_least(Role::Captain)) != Some(true) {
        let message = utils::replace(&no_permission_tpl, "<role>", Role::Captain.name());
        drop(guard);
        return reply(sender, format!("{prefix}{message}"));
    }

    let player_name = core::name_of(&player);

    if target.eq_ignore_ascii_case("all") {
        if let Some(faction) = guard.factions.get_mut(&id) {
            faction.invitations.clear();
        }
        let message = utils::replace(&all_tpl, "<player>", &player_name);
        let members = guard
            .factions
            .get(&id)
            .map(|f| f.members.values().map(|m| m.uuid.clone()).collect::<Vec<_>>())
            .unwrap_or_default();
        drop(guard);
        let message = format!("{prefix}{message}");
        for member_uuid in members {
            if let Some(id) = pumpkin_plugin_api::uuid::parse(&member_uuid)
                && let Some(online) = server.get_player_by_uuid(id)
            {
                core::send(&online, &message);
            }
        }
        return;
    }

    let target_uuid = server.get_player_by_name(&target).map(|p| core::uuid_of(&p));
    let removed = match (target_uuid, guard.factions.get_mut(&id)) {
        (Some(target_uuid), Some(faction)) if faction.invitations.contains(&target_uuid) => {
            faction.invitations.retain(|id| id != &target_uuid);
            true
        }
        _ => false,
    };

    let template = if removed { uninvited_tpl } else { not_invited_tpl };
    let message = format!("{prefix}{}", utils::replace(&template, "<player>", &target));
    let members = guard
        .factions
        .get(&id)
        .map(|f| f.members.values().map(|m| m.uuid.clone()).collect::<Vec<_>>())
        .unwrap_or_default();
    drop(guard);

    if !removed {
        return reply(sender, message);
    }
    for member_uuid in members {
        if let Some(id) = pumpkin_plugin_api::uuid::parse(&member_uuid)
            && let Some(online) = server.get_player_by_uuid(id)
        {
            core::send(&online, &message);
        }
    }
}

fn auto_revive(sender: &CommandSender, server: &Server) {
    let Some(player) = sender.as_player() else {
        return;
    };
    let uuid = core::uuid_of(&player);

    let (message, members) = {
        let mut guard = state().write().unwrap();
        let prefix = guard.raw.language.string("FACTION_PREFIX");
        let not_in = guard.raw.language.string("FACTIONS.NOT_IN_FACTION_SELF");
        let no_perm_tpl = guard.raw.language.string("FACTIONS.NO_PERMISSION");
        let on_tpl = guard.raw.language.string("FACTIONS.AUTO_REVIVE_ENABLED");
        let off_tpl = guard.raw.language.string("FACTIONS.AUTO_REVIVE_DISABLED");

        let Some(id) = guard.factions.by_member(&uuid).map(|f| f.id.clone()) else {
            return reply(sender, format!("{prefix}{not_in}"));
        };

        let role = guard.factions.get(&id).and_then(|f| f.member(&uuid)).map(|m| m.role);
        if role.map(|r| r.is_at_least(Role::CoLeader)) != Some(true) {
            let message = utils::replace(&no_perm_tpl, "<role>", Role::CoLeader.name());
            return reply(sender, format!("{prefix}{message}"));
        }

        let name = core::name_of(&player);
        let faction = guard.factions.get_mut(&id).unwrap();
        faction.auto_revive = !faction.auto_revive;
        let template = if faction.auto_revive { on_tpl } else { off_tpl };
        let members = guard
            .factions
            .get(&id)
            .unwrap()
            .members
            .values()
            .map(|m| m.uuid.clone())
            .collect::<Vec<_>>();
        (
            format!("{prefix}{}", utils::replace(&template, "<player>", &name)),
            members,
        )
    };

    for member_uuid in members {
        if let Some(id) = pumpkin_plugin_api::uuid::parse(&member_uuid)
            && let Some(online) = server.get_player_by_uuid(id)
        {
            core::send(&online, &message);
        }
    }
}

fn claim_chunk(sender: &CommandSender, server: &Server, player: &Player, uuid: &str) {
    let _ = server;
    let messages;
    {
        let mut guard = state().write().unwrap();
        let prefix = guard.raw.language.string("FACTION_PREFIX");
        let location = core::location(player);

        let Some(id) = guard.factions.by_member(uuid).map(|f| f.id.clone()) else {
            let msg = guard.raw.language.string("FACTIONS.NOT_IN_FACTION_SELF");
            drop(guard);
            return reply(sender, format!("{prefix}{msg}"));
        };

        let role = guard.factions.get(&id).and_then(|f| f.member(uuid)).map(|m| m.role);
        if role.map(|r| r.is_at_least(Role::CoLeader)) != Some(true) {
            let msg = utils::replace(
                &guard.raw.language.string("FACTIONS.NO_PERMISSION"),
                "<role>",
                Role::CoLeader.name(),
            );
            drop(guard);
            return reply(sender, format!("{prefix}{msg}"));
        }

        let claim_count = guard.claims.claim_count(&id);
        let max_claims = guard.raw.config.int("FACTION_CLAIM.MAX_CLAIMS") as usize;
        let per_player = guard.raw.config.int("FACTION_CLAIM.CLAIMS_PER_PLAYER") as usize;
        let member_count = guard.factions.get(&id).map(|f| f.members.len()).unwrap_or(1);

        if claim_count >= max_claims || claim_count >= member_count * per_player {
            let msg = guard.raw.language.string("FACTIONS.CLAIM_MAX_CLAIMS_EXCEEDED");
            drop(guard);
            return reply(sender, format!("{prefix}{msg}"));
        }

        if core::dimension_of(&location.world) != core::Dimension::Overworld {
            let msg = guard.raw.language.string("FACTIONS.CLAIM_CAN_CLAIM_ONLY_IN_OVERWORLD");
            drop(guard);
            return reply(sender, format!("{prefix}{msg}"));
        }

        let (cx, cz) = crate::utils::chunk_key(location.block_x(), location.block_z());
        let x1 = cx * 16;
        let z1 = cz * 16;
        let claim = Claim::new(location.world.clone(), x1, x1 + 15, z1, z1 + 15);

        if !crate::listeners::interact::is_selection_connected(&guard.claims, &id, &claim, guard.raw.config.bool("FACTION_CLAIM.MUST_BE_CONNECTED")) {
            let msg = guard.raw.language.string("FACTIONS.CLAIM_CLAIM_NOT_CONNECTED");
            drop(guard);
            return reply(sender, format!("{prefix}{msg}"));
        }

        if guard.claims.overlaps(&claim) {
            let msg = guard.raw.language.string("FACTIONS.CLAIM_CLAIM_OVERLAPPING");
            drop(guard);
            return reply(sender, format!("{prefix}{msg}"));
        }

        let buffer = guard.raw.config.int("FACTION_CLAIM.CLAIM_BUFFER") as i32;
        let ignore_roads = guard.raw.config.bool("FACTION_CLAIM.CLAIM_BUFFER_IGNORE_ROADS");
        let ignore = if ignore_roads { guard.factions.road_ids() } else { Vec::new() };
        if guard.claims.within_buffer(&claim, buffer, &id, &ignore) {
            let msg = utils::replace(
                &guard.raw.language.string("FACTIONS.CLAIM_CLAIM_BUFFER"),
                "<buffer>",
                &buffer.to_string(),
            );
            drop(guard);
            return reply(sender, format!("{prefix}{msg}"));
        }

        let size = claim.size_x() * claim.size_z();
        let price = (size as f64 * guard.raw.config.double("FACTION_CLAIM.CLAIM_PRICE_PER_BLOCK")) as i64;
        let balance = guard.factions.get(&id).map(|f| f.balance).unwrap_or(0);
        if balance < price {
            let msg = guard.raw.language.string("FACTIONS.CLAIM_NOT_ENOUGH_MONEY");
            drop(guard);
            return reply(sender, format!("{prefix}{msg}"));
        }

        guard.claims.add_claim(&id, claim.clone());
        if let Some(faction) = guard.factions.get_mut(&id) {
            faction.balance -= price;
        }

        let player_name = core::name_of(player);
        let msg = utils::replace_all(
            &guard.raw.language.string("FACTIONS.CLAIM_CLAIM_CLAIMED"),
            &[
                ("<player>", &player_name),
                ("<location1>", &format!("{}, {}", claim.min_x, claim.min_z)),
                ("<location2>", &format!("{}, {}", claim.max_x, claim.max_z)),
            ],
        );
        messages = format!("{prefix}{msg}");
        let members = guard
            .factions
            .get(&id)
            .map(|f| f.members.values().map(|m| m.uuid.clone()).collect::<Vec<_>>())
            .unwrap_or_default();
        drop(guard);

        for member_uuid in members {
            if let Some(id) = pumpkin_plugin_api::uuid::parse(&member_uuid)
                && let Some(online) = server.get_player_by_uuid(id)
            {
                core::send(&online, &messages);
            }
        }
        return;
    }
}

fn lives(sender: &CommandSender, args: &ConsumedArgs) {
    let guard = state().read().unwrap();
    let prefix = guard.raw.language.string("FACTION_PREFIX");

    match present_string_arg(args, "faction") {
        None => {
            // `checkConsoleSender(sender)`.
            let Some(player) = sender.as_player() else {
                let template = guard.raw.language.string("COMMANDS.FOR_PLAYERS_ONLY");
                drop(guard);
                return reply(sender, format!("{prefix}{template}"));
            };
            let uuid = core::uuid_of(&player);
            let Some(id) = guard.factions.by_member(&uuid).map(|f| f.id.clone()) else {
                let msg = guard.raw.language.string("FACTIONS.NOT_IN_FACTION_SELF");
                drop(guard);
                return reply(sender, format!("{prefix}{msg}"));
            };
            let amount = guard.factions.get(&id).map(|f| f.lives).unwrap_or(0);
            let msg = utils::replace(
                &guard.raw.language.string("FACTIONS.LIVES_COMMAND.LIVES_SELF"),
                "<amount>",
                &amount.to_string(),
            );
            drop(guard);
            reply(sender, format!("{prefix}{msg}"));
        }
        Some(name) => {
            let Some(faction_id) = search_player_faction(&guard, &name) else {
                let msg = utils::replace(
                    &guard.raw.language.string("FACTIONS.FACTION_DOESNT_EXIST"),
                    "<argument>",
                    &name,
                );
                drop(guard);
                return reply(sender, format!("{prefix}{msg}"));
            };
            let Some(faction) = guard.factions.get(&faction_id) else {
                return;
            };
            // `faction.getName(sender)`: relation colour (enemy for console).
            let viewer_faction = sender
                .as_player()
                .map(|player| core::uuid_of(&player))
                .and_then(|uuid| guard.factions.by_member(&uuid).map(|f| f.id.clone()));
            let display = core::faction_display_name_guard(
                &guard,
                faction,
                viewer_faction.as_deref(),
                &guard.config.relation_colors,
            );
            let msg = utils::replace_all(
                &guard.raw.language.string("FACTIONS.LIVES_COMMAND.LIVES_OTHERS"),
                &[("<faction>", &display), ("<amount>", &faction.lives.to_string())],
            );
            drop(guard);
            reply(sender, format!("{prefix}{msg}"));
        }
    }
}

fn lives_transfer(
    sender: &CommandSender,
    server: &Server,
    uuid: &str,
    args: &ConsumedArgs,
    deposit: bool,
) {
    let _ = server;
    let Some(player) = sender.as_player() else {
        return;
    };

    let mut guard = state().write().unwrap();
    let prefix = guard.raw.language.string("FACTION_PREFIX");
    let not_in = guard.raw.language.string("FACTIONS.NOT_IN_FACTION_SELF");
    let no_perm_tpl = guard.raw.language.string("FACTIONS.NO_PERMISSION");

    let (usage_key, not_enough_key, template_key) = if deposit {
        (
            "FACTIONS.LIVES_DEPOSIT_COMMAND.USAGE",
            "FACTIONS.LIVES_DEPOSIT_COMMAND.NOT_ENOUGH_LIVES",
            "FACTIONS.LIVES_DEPOSIT_COMMAND.DEPOSITED",
        )
    } else {
        (
            "FACTIONS.LIVES_WITHDRAW_COMMAND.USAGE",
            "FACTIONS.LIVES_WITHDRAW_COMMAND.NOT_ENOUGH_LIVES",
            "FACTIONS.LIVES_WITHDRAW_COMMAND.WITHDRAWN",
        )
    };

    let Some(amount_raw) = present_string_arg(args, "amount") else {
        let usage = guard.raw.language.string(usage_key);
        drop(guard);
        return reply(sender, format!("{prefix}{usage}"));
    };
    // `SubCommand#checkNumber`: invalid input uses `COMMANDS.INVALID_NUMBER`.
    let Some(amount) = amount_raw.trim().parse::<i64>().ok() else {
        let invalid = guard.raw.language.string("COMMANDS.INVALID_NUMBER");
        drop(guard);
        return reply(sender, format!("{prefix}{invalid}"));
    };
    let amount = amount.abs() as i32;

    let Some(id) = guard.factions.by_member(uuid).map(|f| f.id.clone()) else {
        drop(guard);
        return reply(sender, format!("{prefix}{not_in}"));
    };

    if !deposit {
        let role = guard.factions.get(&id).and_then(|f| f.member(uuid)).map(|m| m.role);
        if role.map(|r| r.is_at_least(Role::Captain)) != Some(true) {
            let msg = utils::replace(&no_perm_tpl, "<role>", Role::Captain.name());
            drop(guard);
            return reply(sender, format!("{prefix}{msg}"));
        }
    }

    let user_lives = guard.userdata.get(uuid).map(|u| u.lives).unwrap_or(0);
    let faction_lives = guard.factions.get(&id).map(|f| f.lives).unwrap_or(0);

    if deposit && user_lives < amount {
        let msg = utils::replace(
            &guard.raw.language.string(not_enough_key),
            "<amount>",
            &user_lives.to_string(),
        );
        drop(guard);
        return reply(sender, format!("{prefix}{msg}"));
    }
    if !deposit && faction_lives < amount {
        let msg = guard.raw.language.string(not_enough_key);
        drop(guard);
        return reply(sender, format!("{prefix}{msg}"));
    }

    if deposit {
        if let Some(user) = guard.userdata.get_mut(uuid) {
            user.lives -= amount;
        }
        if let Some(faction) = guard.factions.get_mut(&id) {
            faction.lives += amount;
        }
    } else {
        if let Some(faction) = guard.factions.get_mut(&id) {
            faction.lives -= amount;
        }
        if let Some(user) = guard.userdata.get_mut(uuid) {
            user.lives += amount;
        }
    }

    let name = core::name_of(&player);
    let message = format!(
        "{prefix}{}",
        utils::replace_all(
            &guard.raw.language.string(template_key),
            &[("<player>", &name), ("<amount>", &amount.to_string())],
        )
    );
    broadcast_to_faction(server, &guard, &id, &message);
    drop(guard);
}

fn lives_revive(sender: &CommandSender, server: &Server, uuid: &str, args: &ConsumedArgs) {
    let Some(player) = sender.as_player() else {
        return;
    };

    let mut guard = state().write().unwrap();
    let prefix = guard.raw.language.string("FACTION_PREFIX");
    let not_in = guard.raw.language.string("FACTIONS.NOT_IN_FACTION_SELF");
    let no_perm_tpl = guard.raw.language.string("FACTIONS.NO_PERMISSION");

    let Some(target_name) = present_string_arg(args, "player") else {
        let usage = guard
            .raw
            .language
            .string("FACTIONS.LIVES_REVIVE_COMMAND.USAGE");
        drop(guard);
        return reply(sender, format!("{prefix}{usage}"));
    };

    let Some(id) = guard.factions.by_member(uuid).map(|f| f.id.clone()) else {
        drop(guard);
        return reply(sender, format!("{prefix}{not_in}"));
    };
    let role = guard.factions.get(&id).and_then(|f| f.member(uuid)).map(|m| m.role);
    if role.map(|r| r.is_at_least(Role::Captain)) != Some(true) {
        let msg = utils::replace(&no_perm_tpl, "<role>", Role::Captain.name());
        drop(guard);
        return reply(sender, format!("{prefix}{msg}"));
    }

    let Some(target_uuid) = guard.factions.get(&id).and_then(|f| {
        f.members
            .values()
            .find(|m| m.name.eq_ignore_ascii_case(&target_name))
            .map(|m| m.uuid.clone())
    }) else {
        let msg = utils::replace(
            &guard.raw.language.string("FACTIONS.NOT_IN_FACTION_OTHERS"),
            "<player>",
            &target_name,
        );
        drop(guard);
        return reply(sender, format!("{prefix}{msg}"));
    };

    // (state lock is already held — inline the deathban checks instead of the
    // locking `deathban::` helpers.)
    if !guard
        .userdata
        .get(&target_uuid)
        .is_some_and(|user| user.is_deathbanned())
    {
        let msg = utils::replace(
            &guard
                .raw
                .language
                .string("FACTIONS.LIVES_REVIVE_COMMAND.NOT_DEATHBANNED"),
            "<target>",
            &target_name,
        );
        drop(guard);
        return reply(sender, format!("{prefix}{msg}"));
    }

    if guard.factions.get(&id).map(|f| f.lives).unwrap_or(0) <= 0 {
        let msg = guard
            .raw
            .language
            .string("FACTIONS.LIVES_REVIVE_COMMAND.NOT_ENOUGH_LIVES");
        drop(guard);
        return reply(sender, format!("{prefix}{msg}"));
    }

    if let Some(faction) = guard.factions.get_mut(&id) {
        faction.lives -= 1;
    }
    // (`DeathbanManager#revivePlayer`, inlined while the lock is held.)
    if let Some(user) = guard.userdata.get_mut(&target_uuid) {
        user.revive();
    }

    let message = format!(
        "{prefix}{}",
        utils::replace_all(
            &guard.raw.language.string("FACTIONS.LIVES_REVIVE_COMMAND.REVIVED"),
            &[("<player>", &core::name_of(&player)), ("<target>", &target_name)],
        )
    );
    broadcast_to_faction(server, &guard, &id, &message);
    drop(guard);
}

/// `FactionsManager#searchForFaction`: a player faction by name, falling back
/// to the faction of the player with that name (`getPlayerFaction(String)`
/// resolves the name to the player's uuid).
fn search_player_faction(guard: &crate::state::AppState, name: &str) -> Option<String> {
    if let Some(faction) = guard.factions.by_name(name) {
        return faction.is_player().then(|| faction.id.clone());
    }

    let uuid = guard
        .userdata
        .users
        .values()
        .find(|user| user.name.eq_ignore_ascii_case(name))
        .map(|user| user.uuid.clone())?;

    guard.factions.by_member(&uuid).map(|faction| faction.id.clone())
}

fn broadcast_to_faction(server: &Server, guard: &crate::state::AppState, id: &str, message: &str) {
    let members = guard
        .factions
        .get(id)
        .map(|f| f.members.values().map(|m| m.uuid.clone()).collect::<Vec<_>>())
        .unwrap_or_default();
    for member_uuid in members {
        if let Some(uuid) = pumpkin_plugin_api::uuid::parse(&member_uuid)
            && let Some(online) = server.get_player_by_uuid(uuid)
        {
            core::send(&online, message);
        }
    }
}

fn rally(sender: &CommandSender, server: &Server, player: &Player, uuid: &str) {
    let mut guard = state().write().unwrap();
    let prefix = guard.raw.language.string("FACTION_PREFIX");
    let not_in = guard.raw.language.string("FACTIONS.NOT_IN_FACTION_SELF");
    let template = guard.raw.language.string("FACTIONS.RALLY_COMMAND.RALLY_SET");
    let include_y = guard.raw.config.bool("FACTION_RALLY.INCLUDE_Y_COORDINATE");

    let Some(id) = guard.factions.by_member(uuid).map(|f| f.id.clone()) else {
        drop(guard);
        return reply(sender, format!("{prefix}{not_in}"));
    };

    let location = core::location(player);
    if let Some(faction) = guard.factions.get_mut(&id) {
        faction.rally_location = Some(location.clone());
    }

    let loc_str = if include_y {
        format!("{}, {}, {}", location.block_x(), location.y as i32, location.block_z())
    } else {
        format!("{}, {}", location.block_x(), location.block_z())
    };
    let message = format!(
        "{prefix}{}",
        utils::replace_all(
            &template,
            &[("<player>", &core::name_of(player)), ("<location>", &loc_str)],
        )
    );
    broadcast_to_faction(server, &guard, &id, &message);
    drop(guard);
}

fn remove_rally(sender: &CommandSender, server: &Server, player: &Player, uuid: &str) {
    let mut guard = state().write().unwrap();
    let prefix = guard.raw.language.string("FACTION_PREFIX");
    let not_in = guard.raw.language.string("FACTIONS.NOT_IN_FACTION_SELF");
    let not_set = guard
        .raw
        .language
        .string("FACTIONS.REMOVE_RALLY_COMMAND.RALLY_NOT_SET");
    let removed_tpl = guard
        .raw
        .language
        .string("FACTIONS.REMOVE_RALLY_COMMAND.RALLY_REMOVED");

    let Some(id) = guard.factions.by_member(uuid).map(|f| f.id.clone()) else {
        drop(guard);
        return reply(sender, format!("{prefix}{not_in}"));
    };

    let has = guard
        .factions
        .get(&id)
        .and_then(|f| f.rally_location.as_ref())
        .is_some();
    if !has {
        drop(guard);
        return reply(sender, format!("{prefix}{not_set}"));
    }

    if let Some(faction) = guard.factions.get_mut(&id) {
        faction.rally_location = None;
    }
    let message = format!(
        "{prefix}{}",
        utils::replace(&removed_tpl, "<player>", &core::name_of(player))
    );
    broadcast_to_faction(server, &guard, &id, &message);
    drop(guard);
}

fn focus(sender: &CommandSender, server: &Server, uuid: &str, args: &ConsumedArgs) {
    let mut guard = state().write().unwrap();
    let prefix = guard.raw.language.string("FACTION_PREFIX");
    let usage = guard.raw.language.string("FACTIONS.FOCUS_COMMAND.USAGE");
    let not_in = guard.raw.language.string("FACTIONS.NOT_IN_FACTION_SELF");
    let no_perm_tpl = guard.raw.language.string("FACTIONS.NO_PERMISSION");
    let cannot = guard.raw.language.string("FACTIONS.FOCUS_COMMAND.CANNOT_FOCUS");
    let already_tpl = guard
        .raw
        .language
        .string("FACTIONS.FOCUS_COMMAND.ALREADY_FOCUSING");
    let missing_tpl = guard.raw.language.string("FACTIONS.FACTION_DOESNT_EXIST");
    let focused_lines = guard
        .raw
        .language
        .string_list("FACTIONS.FOCUS_COMMAND.FOCUSED");

    let Some(target_name) = present_string_arg(args, "faction") else {
        drop(guard);
        return reply(sender, format!("{prefix}{usage}"));
    };
    let Some(id) = guard.factions.by_member(uuid).map(|f| f.id.clone()) else {
        drop(guard);
        return reply(sender, format!("{prefix}{not_in}"));
    };
    let role = guard.factions.get(&id).and_then(|f| f.member(uuid)).map(|m| m.role);
    if role.map(|r| r.is_at_least(Role::Captain)) != Some(true) {
        let msg = utils::replace(&no_perm_tpl, "<role>", Role::Captain.name());
        drop(guard);
        return reply(sender, format!("{prefix}{msg}"));
    }

    let Some(target_id) = search_player_faction(&guard, &target_name) else {
        let msg = utils::replace(&missing_tpl, "<argument>", &target_name);
        drop(guard);
        return reply(sender, format!("{prefix}{msg}"));
    };
    let (target_name_real, is_ally) = guard
        .factions
        .get(&target_id)
        .map(|f| {
            let is_ally = guard
                .factions
                .get(&id)
                .map(|own| own.is_ally(&f.id))
                .unwrap_or(false);
            (f.name.clone(), is_ally)
        })
        .unwrap_or_default();

    if target_id == id || is_ally {
        drop(guard);
        return reply(sender, format!("{prefix}{cannot}"));
    }
    if guard
        .factions
        .get(&id)
        .and_then(|f| f.focused_faction.as_ref())
        == Some(&target_id)
    {
        let msg = utils::replace(&already_tpl, "<faction>", &target_name_real);
        drop(guard);
        return reply(sender, format!("{prefix}{msg}"));
    }

    if let Some(faction) = guard.factions.get_mut(&id) {
        faction.focused_faction = Some(target_id.clone());
    }
    for line in focused_lines {
        let msg = format!("{prefix}{}", utils::replace(&line, "<faction>", &target_name_real));
        broadcast_to_faction(server, &guard, &id, &msg);
    }
    drop(guard);
}

fn unfocus(sender: &CommandSender, _server: &Server, uuid: &str) {
    let mut guard = state().write().unwrap();
    let prefix = guard.raw.language.string("FACTION_PREFIX");
    let not_in = guard.raw.language.string("FACTIONS.NOT_IN_FACTION_SELF");
    let no_perm_tpl = guard.raw.language.string("FACTIONS.NO_PERMISSION");
    let not_focusing = guard
        .raw
        .language
        .string("FACTIONS.UNFOCUS_COMMAND.NOT_FOCUSING");
    let unfocused_tpl = guard
        .raw
        .language
        .string("FACTIONS.UNFOCUS_COMMAND.UNFOCUSED");
    let missing = guard
        .raw
        .language
        .string("FACTIONS.UNFOCUS_COMMAND.FACTION_DOESNT_EXIST");

    let Some(id) = guard.factions.by_member(uuid).map(|f| f.id.clone()) else {
        drop(guard);
        return reply(sender, format!("{prefix}{not_in}"));
    };
    let role = guard.factions.get(&id).and_then(|f| f.member(uuid)).map(|m| m.role);
    if role.map(|r| r.is_at_least(Role::Captain)) != Some(true) {
        let msg = utils::replace(&no_perm_tpl, "<role>", Role::Captain.name());
        drop(guard);
        return reply(sender, format!("{prefix}{msg}"));
    }

    let focused = guard.factions.get(&id).and_then(|f| f.focused_faction.clone());
    let Some(focused) = focused else {
        drop(guard);
        return reply(sender, format!("{prefix}{not_focusing}"));
    };

    let target_name = guard.factions.get(&focused).map(|f| f.name.clone());
    if let Some(faction) = guard.factions.get_mut(&id) {
        faction.focused_faction = None;
    }

    match target_name {
        Some(name) => {
            let msg = format!("{prefix}{}", utils::replace(&unfocused_tpl, "<faction>", &name));
            drop(guard);
            reply(sender, msg);
        }
        None => {
            drop(guard);
            reply(sender, format!("{prefix}{missing}"));
        }
    }
}

fn admin_remove(sender: &CommandSender, server: &Server, args: &ConsumedArgs) {
    if !require_permission(sender, server, "lazarus:factions.remove") {
        return;
    }

    let (prefix, usage, missing_tpl, invalid_type, sender_tpl, faction_tpl, found) = {
        let guard = state().read().unwrap();
        let lang = &guard.raw.language;
        let found = guard.factions.by_name(&present_string_arg(args, "faction").unwrap_or_default()).map(|f| {
            (
                f.id.clone(),
                f.name.clone(),
                f.is_player(),
                f.members.values().map(|m| m.uuid.clone()).collect::<Vec<_>>(),
            )
        });
        (
            lang.string("FACTION_PREFIX"),
            lang.string("FACTIONS.REMOVE_COMMAND.USAGE"),
            lang.string("FACTIONS.FACTION_DOESNT_EXIST"),
            lang.string("FACTIONS.REMOVE_COMMAND.CANNOT_REMOVE_THIS_TYPE"),
            lang.string("FACTIONS.REMOVE_COMMAND.REMOVED_SENDER"),
            lang.string("FACTIONS.REMOVE_COMMAND.REMOVED_FACTION"),
            found,
        )
    };

    let Some(name) = present_string_arg(args, "faction") else {
        reply(sender, format!("{prefix}{usage}"));
        return;
    };
    let Some((id, faction_name, is_player, members)) = found else {
        let message = utils::replace(&missing_tpl, "<argument>", &name);
        reply(sender, format!("{prefix}{message}"));
        return;
    };
    if !is_player {
        reply(sender, format!("{prefix}{invalid_type}"));
        return;
    }

    {
        let mut guard = state().write().unwrap();
        guard.claims.remove_all(&id);
        guard.factions.disband(&id);
    }

    reply(
        sender,
        format!("{prefix}{}", utils::replace(&sender_tpl, "<faction>", &faction_name)),
    );

    let message = format!(
        "{prefix}{}",
        utils::replace(&faction_tpl, "<player>", &sender.get_name())
    );
    for uuid in members {
        if let Some(id) = pumpkin_plugin_api::uuid::parse(&uuid)
            && let Some(player) = server.get_player_by_uuid(id)
        {
            core::send(&player, &message);
        }
    }
}

fn admin_clear_claims(sender: &CommandSender, server: &Server, args: &ConsumedArgs) {
    if !require_permission(sender, server, "lazarus:factions.clearclaims") {
        return;
    }

    let (prefix, usage, missing_tpl, sender_tpl, faction_tpl, home_tpl, found) = {
        let guard = state().read().unwrap();
        let lang = &guard.raw.language;
        let found = guard.factions.by_name(&present_string_arg(args, "faction").unwrap_or_default()).map(|f| {
            (
                f.id.clone(),
                f.name.clone(),
                f.is_player(),
                f.members.values().map(|m| m.uuid.clone()).collect::<Vec<_>>(),
            )
        });
        (
            lang.string("FACTION_PREFIX"),
            lang.string("FACTIONS.CLEAR_CLAIMS_COMMAND.USAGE"),
            lang.string("FACTIONS.FACTION_DOESNT_EXIST"),
            lang.string("FACTIONS.CLEAR_CLAIMS_COMMAND.CLEARED_SENDER"),
            lang.string("FACTIONS.CLEAR_CLAIMS_COMMAND.CLEARED_FACTION"),
            lang.string("FACTIONS.CLEAR_CLAIMS_COMMAND.HOME_TELEPORT_CANCELLED"),
            found,
        )
    };

    let Some(name) = present_string_arg(args, "faction") else {
        reply(sender, format!("{prefix}{usage}"));
        return;
    };
    let Some((id, faction_name, is_player, members)) = found else {
        let message = utils::replace(&missing_tpl, "<argument>", &name);
        reply(sender, format!("{prefix}{message}"));
        return;
    };

    {
        let mut guard = state().write().unwrap();
        guard.claims.remove_all(&id);
        if is_player {
            if let Some(faction) = guard.factions.get_mut(&id) {
                faction.home = None;
            }
        }
    }

    reply(
        sender,
        format!("{prefix}{}", utils::replace(&sender_tpl, "<faction>", &faction_name)),
    );

    if !is_player {
        return;
    }

    let message = format!(
        "{prefix}{}",
        utils::replace(&faction_tpl, "<player>", &sender.get_name())
    );
    for uuid in members {
        if let Some(id) = pumpkin_plugin_api::uuid::parse(&uuid)
            && let Some(player) = server.get_player_by_uuid(id)
        {
            core::send(&player, &message);
            core::send(&player, format!("{prefix}{home_tpl}"));
        }
    }
}


/// `SetDtrCommand`: usage, `searchForFaction`, `isDouble`, then `setDtr`
/// (clamped to min/max and rounded); messages carry `getDtrString()`.
fn admin_set_dtr(sender: &CommandSender, server: &Server, args: &ConsumedArgs) {
    if !require_permission(sender, server, "lazarus:factions.dtr") {
        return;
    }

    let mut guard = state().write().unwrap();
    let prefix = guard.raw.language.string("FACTION_PREFIX");

    let (Some(name), Some(value_raw)) = (
        present_string_arg(args, "faction"),
        present_string_arg(args, "value"),
    ) else {
        let usage = guard.raw.language.string("FACTIONS.SET_DTR_COMMAND.USAGE");
        drop(guard);
        return reply(sender, format!("{prefix}{usage}"));
    };

    let Some(id) = search_player_faction(&guard, &name) else {
        let tpl = guard.raw.language.string("FACTIONS.FACTION_DOESNT_EXIST");
        let msg = utils::replace(&tpl, "<argument>", &name);
        drop(guard);
        return reply(sender, format!("{prefix}{msg}"));
    };

    let value = match value_raw.trim().parse::<f64>() {
        Ok(value) if value.is_finite() => value,
        _ => {
            let invalid = guard.raw.language.string("COMMANDS.INVALID_NUMBER");
            drop(guard);
            return reply(sender, format!("{prefix}{invalid}"));
        }
    };

    let config = guard.config.clone();
    let (faction_name, members) = {
        let Some(faction) = guard.factions.get_mut(&id) else {
            return;
        };
        faction.set_dtr(&config, value);
        (
            faction.name.clone(),
            faction.members.values().map(|m| m.uuid.clone()).collect::<Vec<_>>(),
        )
    };
    let frozen = guard.timers.is_frozen(&id);
    let dtr_string = guard
        .factions
        .get(&id)
        .map(|faction| faction.dtr_string(&config, frozen))
        .unwrap_or_default();
    let sender_tpl = guard.raw.language.string("FACTIONS.SET_DTR_COMMAND.CHANGED_SENDER");
    let faction_tpl = guard.raw.language.string("FACTIONS.SET_DTR_COMMAND.CHANGED_FACTION");
    drop(guard);

    reply(
        sender,
        format!(
            "{prefix}{}",
            utils::replace_all(&sender_tpl, &[("<faction>", &faction_name), ("<value>", &dtr_string)])
        ),
    );

    let message = format!(
        "{prefix}{}",
        utils::replace_all(
            &faction_tpl,
            &[("<player>", &sender.get_name()), ("<dtr>", &dtr_string)]
        )
    );
    for uuid in members {
        if let Some(id) = pumpkin_plugin_api::uuid::parse(&uuid)
            && let Some(player) = server.get_player_by_uuid(id)
        {
            core::send(&player, &message);
        }
    }
}

fn admin_set_int(sender: &CommandSender, server: &Server, args: &ConsumedArgs, field: AdminField) {
    if !require_permission(
        sender,
        server,
        match field {
            AdminField::Points => "lazarus:factions.setpoints",
            AdminField::Balance => "lazarus:factions.setbalance",
            AdminField::Lives => "lazarus:factions.setlives",
            AdminField::Kills => "lazarus:factions.setkills",
        },
    ) {
        return;
    }

    let mut guard = state().write().unwrap();
    let prefix = guard.raw.language.string("FACTION_PREFIX");
    let usage_key = match field {
        AdminField::Points => "FACTIONS.SET_POINTS_COMMAND.USAGE",
        AdminField::Balance => "FACTIONS.SET_BALANCE_COMMAND.USAGE",
        AdminField::Lives => "FACTIONS.SET_LIVES_COMMAND.USAGE",
        AdminField::Kills => "FACTIONS.SET_KILLS_COMMAND.USAGE",
    };

    // `args.length < 2`.
    let (Some(name), Some(value_raw)) = (
        present_string_arg(args, "faction"),
        present_string_arg(args, "value"),
    ) else {
        let usage = guard.raw.language.string(usage_key);
        drop(guard);
        return reply(sender, format!("{prefix}{usage}"));
    };

    let Some(id) = search_player_faction(&guard, &name) else {
        let tpl = guard.raw.language.string("FACTIONS.FACTION_DOESNT_EXIST");
        let msg = utils::replace(&tpl, "<argument>", &name);
        drop(guard);
        return reply(sender, format!("{prefix}{msg}"));
    };

    let Ok(value) = value_raw.trim().parse::<i64>() else {
        let invalid = guard.raw.language.string("COMMANDS.INVALID_NUMBER");
        drop(guard);
        return reply(sender, format!("{prefix}{invalid}"));
    };

    // `Math.abs` for balance/lives/kills; points keep their sign.
    let amount = match field {
        AdminField::Points => value,
        _ => value.abs(),
    };

    let allow_negative = guard.config.factions.allow_negative_points;
    let (faction_name, members) = {
        let Some(faction) = guard.factions.get_mut(&id) else {
            return;
        };
        match field {
            AdminField::Points => faction.change_points(amount as i32, allow_negative),
            AdminField::Balance => faction.balance = amount,
            AdminField::Lives => faction.lives = amount as i32,
            AdminField::Kills => faction.kills = amount as i32,
        }
        (
            faction.name.clone(),
            faction.members.values().map(|m| m.uuid.clone()).collect::<Vec<_>>(),
        )
    };

    let amount = amount.to_string();
    let sender_tpl = guard.raw.language.string(match field {
        AdminField::Points => "FACTIONS.SET_POINTS_COMMAND.CHANGED_SENDER",
        AdminField::Balance => "FACTIONS.SET_BALANCE_COMMAND.CHANGED_SENDER",
        AdminField::Lives => "FACTIONS.SET_LIVES_COMMAND.CHANGED_SENDER",
        AdminField::Kills => "FACTIONS.SET_KILLS_COMMAND.CHANGED_SENDER",
    });
    let faction_tpl = guard.raw.language.string(match field {
        AdminField::Points => "FACTIONS.SET_POINTS_COMMAND.CHANGED_FACTION",
        AdminField::Balance => "FACTIONS.SET_BALANCE_COMMAND.CHANGED_FACTION",
        AdminField::Lives => "FACTIONS.SET_LIVES_COMMAND.CHANGED_FACTION",
        AdminField::Kills => "FACTIONS.SET_KILLS_COMMAND.CHANGED_FACTION",
    });
    drop(guard);

    reply(
        sender,
        format!(
            "{prefix}{}",
            utils::replace_all(&sender_tpl, &[("<faction>", &faction_name), ("<amount>", &amount)])
        ),
    );

    let message = format!(
        "{prefix}{}",
        utils::replace_all(
            &faction_tpl,
            &[("<player>", &sender.get_name()), ("<amount>", &amount)]
        )
    );
    for uuid in members {
        if let Some(id) = pumpkin_plugin_api::uuid::parse(&uuid)
            && let Some(player) = server.get_player_by_uuid(id)
        {
            core::send(&player, &message);
        }
    }
}

/// `ForceKickCommand`: usage, then force-kick an online member (the original
/// resolves offline players too; the port keeps the online-only resolution).
fn admin_force_kick(sender: &CommandSender, server: &Server, args: &ConsumedArgs) {
    if !require_permission(sender, server, "lazarus:factions.forcekick") {
        return;
    }

    let Some(name) = present_string_arg(args, "player") else {
        let guard = state().read().unwrap();
        let prefix = guard.raw.language.string("FACTION_PREFIX");
        let usage = guard.raw.language.string("FACTIONS.FORCE_KICK_COMMAND.USAGE");
        drop(guard);
        return reply(sender, format!("{prefix}{usage}"));
    };

    let Some(target) = server.get_player_by_name(&name) else {
        reply(sender, utils::replace(&state().read().unwrap().lang.player_not_online, "<player>", &name));
        return;
    };
    let uuid = core::uuid_of(&target);
    let player_name = core::name_of(&target);

    let messages;
    {
        let mut guard = state().write().unwrap();
        let prefix = guard.raw.language.string("FACTION_PREFIX");

        let Some(faction_id) = guard.factions.by_member(&uuid).map(|f| f.id.clone()) else {
            let tpl = guard.raw.language.string("FACTIONS.NOT_IN_FACTION");
            let msg = utils::replace(&tpl, "<player>", &player_name);
            drop(guard);
            return reply(sender, format!("{prefix}{msg}"));
        };

        let (faction_name, role, members) = {
            let Some(faction) = guard.factions.get(&faction_id) else {
                return;
            };
            (
                faction.name.clone(),
                faction.member(&uuid).map(|m| m.role),
                faction.members.values().map(|m| m.uuid.clone()).collect::<Vec<_>>(),
            )
        };

        if role == Some(Role::Leader) {
            let msg = guard
                .raw
                .language
                .string("FACTIONS.FORCE_KICK_COMMAND.CANNOT_KICK_LEADER");
            drop(guard);
            return reply(sender, format!("{prefix}{msg}"));
        }

        guard.factions.remove_member(&faction_id, &uuid);
        if guard
            .factions
            .get(&faction_id)
            .is_some_and(|f| f.members.is_empty())
        {
            guard.claims.remove_all(&faction_id);
            guard.factions.disband(&faction_id);
        }

        messages = (
            prefix.clone(),
            utils::replace_all(
                &guard.raw.language.string("FACTIONS.FORCE_KICK_COMMAND.KICKED_SENDER"),
                &[("<player>", &player_name), ("<faction>", &faction_name)],
            ),
            utils::replace(
                &guard.raw.language.string("FACTIONS.FORCE_KICK_COMMAND.KICKED_SELF"),
                "<name>",
                &faction_name,
            ),
            utils::replace(
                &guard.raw.language.string("FACTIONS.FORCE_KICK_COMMAND.KICKED_OTHERS"),
                "<player>",
                &player_name,
            ),
            members
                .into_iter()
                .filter(|member_uuid| member_uuid != &uuid)
                .collect::<Vec<_>>(),
        );
    }

    let (prefix, sender_msg, self_msg, others_msg, members) = messages;
    reply(sender, format!("{prefix}{sender_msg}"));
    core::send(&target, format!("{prefix}{self_msg}"));
    for member_uuid in members {
        if let Some(uuid) = pumpkin_plugin_api::uuid::parse(&member_uuid)
            && let Some(online) = server.get_player_by_uuid(uuid)
        {
            core::send(&online, format!("{prefix}{others_msg}"));
        }
    }
}

/// `ForceLeaderCommand`: usage, then promote an online member to leader
/// (old leaders become co-leaders). The original resolves offline players
/// too; the port keeps the online-only resolution.
fn admin_force_leader(sender: &CommandSender, server: &Server, args: &ConsumedArgs) {
    if !require_permission(sender, server, "lazarus:factions.forceleader") {
        return;
    }

    let Some(name) = present_string_arg(args, "player") else {
        let guard = state().read().unwrap();
        let prefix = guard.raw.language.string("FACTION_PREFIX");
        let usage = guard.raw.language.string("FACTIONS.FORCE_LEADER_COMMAND.USAGE");
        drop(guard);
        return reply(sender, format!("{prefix}{usage}"));
    };

    let Some(target) = server.get_player_by_name(&name) else {
        reply(sender, utils::replace(&state().read().unwrap().lang.player_not_online, "<player>", &name));
        return;
    };
    let uuid = core::uuid_of(&target);
    let player_name = core::name_of(&target);
    let sender_name = sender.get_name();

    let messages;
    {
        let mut guard = state().write().unwrap();
        let prefix = guard.raw.language.string("FACTION_PREFIX");

        let Some(faction_id) = guard.factions.by_member(&uuid).map(|f| f.id.clone()) else {
            let tpl = guard.raw.language.string("FACTIONS.NOT_IN_FACTION");
            let msg = utils::replace(&tpl, "<player>", &player_name);
            drop(guard);
            return reply(sender, format!("{prefix}{msg}"));
        };

        let (faction_name, already_leader, members) = {
            let Some(faction) = guard.factions.get(&faction_id) else {
                return;
            };
            (
                faction.name.clone(),
                faction.member(&uuid).map(|m| m.role) == Some(Role::Leader),
                faction.members.values().map(|m| m.uuid.clone()).collect::<Vec<_>>(),
            )
        };

        if already_leader {
            let tpl = guard.raw.language.string("FACTIONS.FORCE_LEADER_COMMAND.ALREADY_LEADER");
            let msg = utils::replace(&tpl, "<player>", &player_name);
            drop(guard);
            return reply(sender, format!("{prefix}{msg}"));
        }

        // `faction.getLeader().setRole(CO_LEADER); targetPlayer.setRole(LEADER);`
        if let Some(faction) = guard.factions.get_mut(&faction_id) {
            for member in faction.members.values_mut() {
                if member.role == Role::Leader {
                    member.role = Role::CoLeader;
                }
            }
            if let Some(member) = faction.members.get_mut(&uuid) {
                member.role = Role::Leader;
            }
        }

        messages = (
            prefix,
            utils::replace_all(
                &guard.raw.language.string("FACTIONS.FORCE_LEADER_COMMAND.CHANGED_SENDER"),
                &[("<player>", &player_name), ("<faction>", &faction_name)],
            ),
            utils::replace_all(
                &guard.raw.language.string("FACTIONS.FORCE_LEADER_COMMAND.CHANGED_FACTION"),
                &[("<sender>", &sender_name), ("<player>", &player_name)],
            ),
            members,
        );
    }

    let (prefix, sender_msg, faction_msg, members) = messages;
    reply(sender, format!("{prefix}{sender_msg}"));
    let message = format!("{prefix}{faction_msg}");
    for member_uuid in members {
        if let Some(uuid) = pumpkin_plugin_api::uuid::parse(&member_uuid)
            && let Some(online) = server.get_player_by_uuid(uuid)
        {
            core::send(&online, &message);
        }
    }
}

fn admin_set_freeze(sender: &CommandSender, server: &Server, args: &ConsumedArgs) {
    if !require_permission(sender, server, "lazarus:factions.setfreeze") {
        return;
    }

    let mut guard = state().write().unwrap();
    let prefix = guard.raw.language.string("FACTION_PREFIX");

    let (Some(name), Some(time_raw)) = (
        present_string_arg(args, "faction"),
        present_string_arg(args, "time"),
    ) else {
        let usage = guard.raw.language.string("FACTIONS.SET_FREEZE_COMMAND.USAGE");
        drop(guard);
        return reply(sender, format!("{prefix}{usage}"));
    };

    let Some(id) = search_player_faction(&guard, &name) else {
        let tpl = guard.raw.language.string("FACTIONS.FACTION_DOESNT_EXIST");
        let msg = utils::replace(&tpl, "<argument>", &name);
        drop(guard);
        return reply(sender, format!("{prefix}{msg}"));
    };

    let Some(duration) = utils::parse_seconds(&time_raw) else {
        let invalid = guard.raw.language.string("COMMANDS.INVALID_DURATION");
        drop(guard);
        return reply(sender, format!("{prefix}{invalid}"));
    };

    let (faction_name, members) = {
        let Some(faction) = guard.factions.get(&id) else {
            return;
        };
        (
            faction.name.clone(),
            faction.members.values().map(|m| m.uuid.clone()).collect::<Vec<_>>(),
        )
    };

    // `timer.cancel(faction)`; a zero duration thaws immediately.
    guard.timers.unfreeze_faction(&id);
    if duration > 0 {
        guard.timers.freeze_faction(&id, duration as u64);
    }

    let time = utils::format_duration_words(duration * 1000);
    let sender_tpl = guard.raw.language.string("FACTIONS.SET_FREEZE_COMMAND.CHANGED_SENDER");
    let faction_tpl = guard.raw.language.string("FACTIONS.SET_FREEZE_COMMAND.CHANGED_FACTION");
    drop(guard);

    reply(
        sender,
        format!(
            "{prefix}{}",
            utils::replace_all(&sender_tpl, &[("<faction>", &faction_name), ("<time>", &time)])
        ),
    );

    let message = format!(
        "{prefix}{}",
        utils::replace_all(&faction_tpl, &[("<player>", &sender.get_name()), ("<time>", &time)])
    );
    for uuid in members {
        if let Some(id) = pumpkin_plugin_api::uuid::parse(&uuid)
            && let Some(player) = server.get_player_by_uuid(id)
        {
            core::send(&player, &message);
        }
    }
}

/// `ThawCommand`: usage, `searchForFaction`, cancel the freeze timer and
/// restart DTR regen immediately.
fn admin_thaw(sender: &CommandSender, server: &Server, args: &ConsumedArgs) {
    if !require_permission(sender, server, "lazarus:factions.thaw") {
        return;
    }

    let mut guard = state().write().unwrap();
    let prefix = guard.raw.language.string("FACTION_PREFIX");

    let Some(name) = present_string_arg(args, "faction") else {
        let usage = guard.raw.language.string("FACTIONS.THAW_COMMAND.USAGE");
        drop(guard);
        return reply(sender, format!("{prefix}{usage}"));
    };

    let Some(id) = search_player_faction(&guard, &name) else {
        let tpl = guard.raw.language.string("FACTIONS.FACTION_DOESNT_EXIST");
        let msg = utils::replace(&tpl, "<argument>", &name);
        drop(guard);
        return reply(sender, format!("{prefix}{msg}"));
    };

    let (faction_name, members) = {
        let Some(faction) = guard.factions.get(&id) else {
            return;
        };
        (
            faction.name.clone(),
            faction.members.values().map(|m| m.uuid.clone()).collect::<Vec<_>>(),
        )
    };

    guard.timers.unfreeze_faction(&id);

    let sender_tpl = guard.raw.language.string("FACTIONS.THAW_COMMAND.CHANGED_SENDER");
    let faction_tpl = guard.raw.language.string("FACTIONS.THAW_COMMAND.CHANGED_FACTION");
    drop(guard);

    reply(
        sender,
        format!(
            "{prefix}{}",
            utils::replace_all(&sender_tpl, &[("<faction>", &faction_name)])
        ),
    );

    let message = format!(
        "{prefix}{}",
        utils::replace_all(&faction_tpl, &[("<player>", &sender.get_name())])
    );
    for uuid in members {
        if let Some(id) = pumpkin_plugin_api::uuid::parse(&uuid)
            && let Some(player) = server.get_player_by_uuid(id)
        {
            core::send(&player, &message);
        }
    }
}

fn admin_save(sender: &CommandSender, server: &Server) {
    if !require_permission(sender, server, "lazarus:factions.save") {
        return;
    }
    crate::state::save_all();
    reply(sender, "&aAll data saved.");
}

/// `HelpCommand`: pages come from `FACTIONS.HELP_COMMAND.PAGES.<page>` in the
/// raw language config; a non-numeric page is `COMMANDS.INVALID_NUMBER` and a
/// missing page is `FACTIONS.HELP_COMMAND.PAGE_NOT_FOUND`.
fn help(sender: &CommandSender, requested: Option<String>) {
    let guard = state().read().unwrap();
    let prefix = guard.raw.language.string("FACTION_PREFIX");

    // `SubCommand#checkNumber` on the raw page argument.
    let page: i64 = match &requested {
        None => 1,
        Some(raw) => match raw.trim().parse::<i64>() {
            Ok(value) => value,
            Err(_) => {
                let invalid = guard.raw.language.string("COMMANDS.INVALID_NUMBER");
                drop(guard);
                return reply(sender, format!("{prefix}{invalid}"));
            }
        },
    };

    let lines = if page >= 1 {
        guard
            .raw
            .language
            .string_list(&format!("FACTIONS.HELP_COMMAND.PAGES.{page}"))
    } else {
        Vec::new()
    };

    if lines.is_empty() {
        let template = guard
            .raw
            .language
            .string("FACTIONS.HELP_COMMAND.PAGE_NOT_FOUND");
        let display = requested.unwrap_or_else(|| page.to_string());
        drop(guard);
        return reply(sender, format!("{prefix}{}", utils::replace(&template, "<page>", &display)));
    }

    drop(guard);
    for line in lines {
        reply(sender, line);
    }
}
